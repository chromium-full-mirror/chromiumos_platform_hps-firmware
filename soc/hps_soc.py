# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""SoC definition and builder script for HPS proto0"""

import argparse
import os

from litex.soc.integration.builder import Builder, builder_args, builder_argdict
from litex.soc.integration.soc import LiteXSoC, SoCRegion
from litex.soc.interconnect import wishbone
from litex.soc.cores.led import LedChaser
from litex.soc.cores.spi_flash import SpiFlash
from litex.build.lattice.radiant import (radiant_build_args,
                                         radiant_build_argdict)

from litespi.modules import GD25LQ128D
from litespi.opcodes import SpiNorFlashOpCodes as Codes
from litespi.phy.generic import LiteSPIPHY
from litespi import LiteSPI

from .hps_platform import Platform
from .cam_control import CameraControl, CameraControlDMA
from .spi_controller import SpiController


KB = 1024
MB = 1024 * KB

UART_SPEED = 460800
RAM_SIZE = 320 * KB

SOC_DIR = os.path.dirname(os.path.realpath(__file__))


class HpsSoC(LiteXSoC):
    """SoC definition for HPS proto0"""
    # Memory layout
    csr_origin = 0xf0000000
    spiflash_region = SoCRegion(0x20000000, 16 * MB, cached=True)
    # The start of the SPI Flash contains the FPGA gateware. Our ROM is after
    # that.
    rom_offset = 2 * MB
    rom_origin = spiflash_region.origin + rom_offset
    rom_region = SoCRegion(rom_origin, spiflash_region.size - rom_offset,
                           cached=True, linker=True)
    sram_origin = 0x40000000
    sram_region = SoCRegion(sram_origin, RAM_SIZE, cached=True, linker=True)
    vexriscv_region = SoCRegion(origin=0xf00f0000, size=0x100)

    # These variables are needed by builder.py. Normally they're defined by
    # SoCCore, but we don't inherit from SoCCore.
    integrated_rom_initialized = False
    integrated_rom_size = rom_region.size

    mem_map = {
        'rom': rom_offset,
        'sram': sram_origin,
        'csr': csr_origin,
    }

    cpu_type = 'vexriscv'

    def __init__(self, *, platform, debug,
                 use_camera_dma=False, litespi_flash=True, litespi_quad=False):
        LiteXSoC.__init__(self,
                          platform=platform,
                          sys_clk_freq=platform.sys_clk_freq,
                          csr_data_width=32 if litespi_flash else 8)

        # Clock, Controller, CPU
        self.submodules.crg = platform.create_crg()
        self.add_controller('ctrl')
        self.add_cpu(self.cpu_type,
                     variant=('full+debug' if debug else 'full'),
                     reset_address=self.rom_origin)

        # RAM
        self.setup_ram()

        # SPI Flash
        if litespi_flash:
            self.setup_litespi_flash(quad=litespi_quad)
        else:
            self.setup_flash()

        # ROM (part of SPI Flash)
        self.bus.add_region('rom', self.rom_region)

        # "LEDS" - Just one LED on JTAG port
        self.submodules.leds = LedChaser(
            pads=platform.request_all('user_led'),
            sys_clk_freq=platform.sys_clk_freq)
        self.csr.add('leds')

        # UART
        self.add_serial()

        # Wishbone UART and CPU debug - JTAG must be disabled to use serial2
        if debug:
            self.add_uartbone('serial2', baudrate=UART_SPEED)
            self.bus.add_slave(
                'vexriscv_debug', self.cpu.debug_bus, self.vexriscv_region)

        # Timer
        self.add_timer(name='timer0')
        self.timer0.add_uptime()

        # Camera
        self.setup_camera(use_camera_dma)

        # SPI controller
        self.submodules.mcu_spi = self.create_spi_controller(
            self.platform.request('mcu_spi', 0))
        self.csr.add('mcu_spi')

    def setup_ram(self):
        self.submodules.lram = self.platform.create_ram(
            32, self.sram_region.size)
        self.bus.add_slave('sram_lram', self.lram.bus, self.sram_region)
        self.bus.add_region('sram', self.sram_region)

    def setup_flash(self):
        self.submodules.spiflash = SpiFlash(self.platform.request('spiflash'),
                                            dummy=8, endianness='little', div=4)
        self.bus.add_slave('spiflash', self.spiflash.bus, self.spiflash_region)
        self.csr.add('spiflash')

    def setup_litespi_flash(self, quad=False):
        read_opcode = Codes.READ_1_1_4 if quad else Codes.READ_1_1_1
        self.submodules.spiflash_phy = LiteSPIPHY(
            self.platform.request('spiflash4x'),
            GD25LQ128D(read_opcode),
            default_divisor=0)
        self.submodules.spiflash_mmap = LiteSPI(
            phy=self.spiflash_phy,
            clk_freq=self.platform.sys_clk_freq,
            mmap_endianness=self.cpu.endianness)
        self.csr.add('spiflash_mmap')
        self.csr.add('spiflash_phy')
        self.bus.add_slave(
            name='spiflash',
            slave=self.spiflash_mmap.bus,
            region=self.spiflash_region)

    def add_serial(self):
        self.add_uart('serial', baudrate=UART_SPEED)

    def setup_camera(self, use_dma):
        if use_dma:
            bus = wishbone.Interface(
                data_width=self.bus.data_width,
                adr_width=self.bus.address_width)
            self.submodules.cam_control = CameraControlDMA(
                bus=bus,
                pads=self.platform.request('cam_control'),
                sys_clk_freq=self.platform.sys_clk_freq)
            self.bus.add_master('cam_control', master=bus)
        else:
            self.submodules.cam_control = CameraControl(
                pads=self.platform.request('cam_control'),
                sys_clk_freq=self.platform.sys_clk_freq)
        self.csr.add('cam_control')

    def create_spi_controller(self, pads):
        return SpiController(pads, self.platform.sys_clk_freq)

    # This method is defined on SoCCore and the builder assumes it exists.
    def initialize_rom(self, data):
        pass

    @property
    def mem_regions(self):
        return self.bus.regions

    def do_finalize(self):
        super().do_finalize()
        # Retro-compatibility for builder
        # TODO: just fix the builder
        for region in self.bus.regions.values():
            region.length = region.size
            region.type = 'cached' if region.cached else 'io'
            if region.linker:
                region.type += '+linker'
        self.csr_regions = self.csr.regions

        self.configure_rom()

    def configure_rom(self):
        # Have the builder build the BIOS, but not create an integrated ROM
        self.integrated_rom_size = 0
        self.integrated_rom_initialized = False


class HpsBuilder(Builder):
    """Object which coordinates the build process, with HPS customizations."""

    def __init__(self, soc, **kwargs):
        super().__init__(soc, **kwargs)
        # TODO(dcallagh): we need to stop pretending our application is a BIOS,
        # this is getting too messy...
        litex_soc_dir = f'{SOC_DIR}/../third_party/python/litex/litex/soc'
        self.software_packages = [
            ('libcompiler_rt', f'{litex_soc_dir}/software/libcompiler_rt'),
            ('libbase', f'{litex_soc_dir}/software/libbase'),
            ('bios', f'{SOC_DIR}/software/bios'),
        ]
        self.software_libraries = [
            'libcompiler_rt',
            'libbase-nofloat',
        ]

    def add_software_package(self, name, src_dir=None):
        # Discard it, we specify our own in __init__
        pass

    def add_software_library(self, name):
        # Discard it, we specify our own in __init__
        pass


def hps_soc_args(parser: argparse.ArgumentParser):
    builder_args(parser)
    radiant_build_args(parser)


def create_builder(soc, args):
    builder = HpsBuilder(soc, **builder_argdict(args))
    builder.output_dir = args.output_dir
    return builder


def main():
    parser = argparse.ArgumentParser(description='HPS SoC')
    hps_soc_args(parser)
    parser.add_argument(
        '--debug', action='store_true',
        help='Enable debug mode')
    parser.add_argument(
        '--no-camera-dma', dest='use_camera_dma',
        action='store_false', default=True,
        help='Do not generate DMA writer interface for camera')
    parser.add_argument(
        '--build', action='store_true',
        help='Whether to do a full build, including the bitstream')
    parser.add_argument(
        '--toolchain', default='radiant',
        help='Which toolchain to use, radiant (default) or oxide')
    parser.add_argument(
        '--cache', default=None,
        help='Use custom cache config: 8d8i2w, 8d8i4w [default: 4d4i1w]')
    parser.add_argument(
        '--no-litespi-flash', dest='litespi_flash',
        action='store_false', default=True,
        help='Use Litex minimal SPI flash instead of Litespi')
    parser.add_argument(
        '--litespi-quad', action='store_true',
        help='Use quad read opcode when reading from SPI flash')

    args = parser.parse_args()

    soc = HpsSoC(
        platform=Platform(args.toolchain),
        debug=args.debug,
        use_camera_dma=args.use_camera_dma,
        litespi_flash=args.litespi_flash,
        litespi_quad=args.litespi_quad)

    if args.cache:
        if args.output_dir is None:
            args.output_dir = ('build/hps_' + args.cache)
            print(f'Set output dir to {args.output_dir}')
        assert 'full' in soc.cpu.variant
        assert 'debug' not in soc.cpu.variant    # we can add this if wanted
        vexriscv = f'{SOC_DIR}/../third_party/python/' \
                   f'pythondata-cpu-vexriscv/pythondata_cpu_vexriscv'
        var = 'Full'
        cache = args.cache
        soc.cpu.use_external_variant(
            f'{vexriscv}/verilog/VexRiscv_{var}{cache}.v')
    else:
        if args.output_dir is None:
            args.output_dir = 'build/hps_platform'

    builder = create_builder(soc, args)
    builder_kwargs = radiant_build_argdict(
        args) if args.toolchain == 'radiant' else {}
    vns = builder.build(**builder_kwargs, run=args.build)
    soc.do_exit(vns)

    if not args.build:
        print('Use --build to build the bitstream, if needed')

    print(f'Loading from output dir: {builder.output_dir}')
    os.system(
        f'scripts/prog bitstream '
        f'{builder.output_dir}/gateware/hps_platform.bit')
    os.system(
        f'scripts/prog rom '
        f'{builder.output_dir}/software/bios/out/riscv-gcc/rom.bin')


if __name__ == '__main__':
    main()
