# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""SoC definition and builder script for HPS"""

import argparse
import os
import re
import time
import subprocess

import pkg_resources

from litex.soc.integration.builder import Builder, builder_args, builder_argdict
from litex.soc.integration.soc import LiteXSoC, SoCRegion
from litex.soc.cores.led import LedChaser

from litespi.modules import GD25LQ128D
from litespi.opcodes import SpiNorFlashOpCodes as Codes
from litespi.phy.generic import LiteSPIPHY
from litespi import LiteSPI

from .hps_platform import Platform
from .cam_control import CameraControl
from .patch_cpu_variant import patch_cpu_variant
from .spi_controller import SpiController


KB = 1024
MB = 1024 * KB

RAM_SIZE = 320 * KB

SOC_DIR = os.path.dirname(os.path.realpath(__file__))


class HpsSoC(LiteXSoC):
    """SoC definition for HPS"""
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

    def __init__(self, *, platform, cpu_cfu=None, variant=None,
                 litespi_flash=True):
        LiteXSoC.__init__(self,
                          platform=platform,
                          sys_clk_freq=platform.sys_clk_freq,
                          csr_data_width=32 if litespi_flash else 8)

        # Clock, Controller, CPU
        self.submodules.crg = platform.create_crg()
        self.add_controller('ctrl')
        self.add_cpu(self.cpu_type,
                     variant=variant,
                     reset_address=self.rom_origin,
                     cfu=cpu_cfu)

        # RAM
        self.setup_ram()

        # SPI Flash
        self.setup_litespi_flash()

        # ROM (part of SPI Flash)
        self.bus.add_region('rom', self.rom_region)

        # "LEDS" - Just one LED on JTAG port
        self.submodules.leds = LedChaser(
            pads=platform.request_all('user_led'),
            sys_clk_freq=platform.sys_clk_freq)
        self.csr.add('leds')

        # Timer
        self.add_timer(name='timer0')
        self.timer0.add_uptime()

        # Camera
        self.setup_camera()

        # SPI controller
        self.submodules.mcu_spi = self.create_spi_controller(
            self.platform.request('mcu_spi', 0))
        self.csr.add('mcu_spi')

    def setup_ram(self):
        self.submodules.lram = self.platform.create_ram(
            32, self.sram_region.size)
        self.bus.add_slave('sram_lram', self.lram.bus, self.sram_region)
        self.bus.add_region('sram', self.sram_region)

    def setup_litespi_flash(self):
        self.submodules.spiflash_phy = LiteSPIPHY(
            self.platform.request('spiflash4x'),
            GD25LQ128D(Codes.READ_1_1_4),
            default_divisor=0,
            rate='1:2',
            extra_latency=1)
        self.submodules.spiflash_mmap = LiteSPI(
            phy=self.spiflash_phy,
            mmap_endianness=self.cpu.endianness)
        self.csr.add('spiflash_mmap')
        self.csr.add('spiflash_phy')
        self.bus.add_slave(
            name='spiflash',
            slave=self.spiflash_mmap.bus,
            region=self.spiflash_region)

    def setup_camera(self):
        self.submodules.cam_control = CameraControl(
            pads=self.platform.request('cam_control'))
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
        # We don't want to build the LiteX BIOS since we don't use it.
        self.integrated_rom_size = 0
        self.integrated_rom_initialized = True


def hps_soc_args(parser: argparse.ArgumentParser):
    builder_args(parser)
    parser.add_argument('--nextpnr-seed', metavar='NUMBER',
                        default=1, type=int,
                        help='Use random seed NUMBER for nextpnr')


def create_builder(soc, args):
    builder = Builder(soc, **builder_argdict(args))
    builder.compile_software = False
    builder.output_dir = args.output_dir
    return builder


def parse_metrics(output_filename):
    """Scrapes interesting metrics from the FPGA toolchain output."""
    with open(output_filename, 'r', encoding='latin-1') as f:
        output_lines = f.readlines()
    metrics = {}
    # Find the percentage utilisation of some important FPGA resources.
    for resource_type in ['OXIDE_FF', 'OXIDE_COMB', 'OXIDE_EBR', 'LRAM_CORE']:
        for line in output_lines:
            m = re.search(rf'{resource_type}:.* (\d+)%$', line)
            if m:
                metrics[f'{resource_type}_PERCENT'] = m.group(1)
    # Find the estimated max frequency. Note that the message is printed twice
    # and only the final one matters.
    for line in reversed(output_lines):
        m = re.search(r'Max frequency for clock.*: (\d+\.\d+) MHz ', line)
        if m:
            metrics['fMAX_MHz'] = m.group(1)
            break
    return metrics


def main():
    patch_cpu_variant()

    parser = argparse.ArgumentParser(description='HPS SoC')
    hps_soc_args(parser)
    # --build does nothing, accept it for backwards compatibility
    parser.add_argument(
        '--build', action='store_true',
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--no-build', action='store_true',
        help='Skip building gateware. This is useful if you just want to ' +
        'quickly regenerate SVD files or associated artifacts.')
    parser.add_argument(
        '--parallel-nextpnr', action='store_true',
        help='Runs $(nproc) copies of nextpnr in parallel. Used to search ' +
        'for a seed.')
    parser.add_argument(
        '--no-litespi-flash', dest='litespi_flash',
        action='store_false', default=True,
        help='Use Litex minimal SPI flash instead of Litespi.')
    parser.add_argument(
        '--cpu-cfu', default='soc/cfu.v',
        help='Specify file containing CFU Verilog module.')

    args = parser.parse_args()
    args.output_dir = 'build/hps_platform'

    variant, variantName = 'full', 'Full'
    if args.cpu_cfu:
        variant, variantName = 'slim+cfu', 'SlimCfu'
    print(f'{variantName} variant selected')

    soc = HpsSoC(
        platform=Platform(parallel_nextpnr=args.parallel_nextpnr),
        litespi_flash=args.litespi_flash,
        variant=variant,
        cpu_cfu=args.cpu_cfu)

    soc.cpu.use_external_variant(
        pkg_resources.resource_filename('pythondata_cpu_vexriscv',
                                        f'verilog/VexRiscv_{variantName}.v'))

    args.csr_svd = f'{args.output_dir}/hps.svd'
    args.memory_x = f'{args.output_dir}/memory.x'

    builder = create_builder(soc, args)
    builder_kwargs = {
        'abc9': True,
        'timingstrict': True,
        'seed': args.nextpnr_seed,
    }

    start_time = time.time()
    vns = builder.build(**builder_kwargs, run=not args.no_build)
    run_time = time.time() - start_time
    output_log = f'{builder.output_dir}/gateware/hps_platform_output.log'
    metrics = parse_metrics(output_log)
    meta_file = f'{builder.output_dir}/gateware/hps_platform_build.metadata'
    print(f'Execution time for the build is: {run_time}s')
    with open(meta_file, 'w', encoding='latin-1') as f:
        f.write(f'RUN_TIME: {run_time}\n')
        for name, value in metrics.items():
            f.write(f'{name}: {value}\n')

    soc.do_exit(vns)

    # Generate the peripheral access crate (PAC) containing Rust code to use the
    # features of our SOC.
    pac_out_dir = f'{builder.output_dir}/litex_pac'
    os.makedirs(pac_out_dir, exist_ok=True)
    subprocess.check_call(['svd2rust',
                           '--target', 'riscv',
                           '-i', f'{builder.output_dir}/hps.svd',
                           '--output-dir', pac_out_dir])
    subprocess.check_call(
        ['rustfmt', f'{pac_out_dir}/lib.rs', f'{pac_out_dir}/build.rs'])

    print(f'Build outputs at {builder.output_dir}')


if __name__ == '__main__':
    main()
