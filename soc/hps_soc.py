# Copyright 2021 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""SoC definition and builder script for HPS"""

import argparse
import os
import re
import subprocess
import sys
import time
import types

import amaranth.back.verilog
from litespi import LiteSPI
from litespi.modules import GD25LQ128D
from litespi.opcodes import SpiNorFlashOpCodes as Codes
from litespi.phy.generic import LiteSPIPHY
from litex.soc.cores.cpu import CPUS
from litex.soc.cores.cpu.vexriscv import VexRiscv
from litex.soc.integration.builder import Builder
from litex.soc.integration.builder import builder_argdict
from litex.soc.integration.builder import builder_args
from litex.soc.integration.soc import LiteXSoC
from litex.soc.integration.soc import SoCRegion
from litex.soc.interconnect import wishbone
from migen import ClockDomain
from migen import ClockDomainsRenamer
from migen import ClockSignal
from migen import Instance
from migen import Record
from migen import ResetSignal

from .cam_control import CameraControl
from .cfu.gen2 import hps_cfu
from .clock_control import CfuCpuClockCtrl
from .hps_platform import Platform
from .spi_controller import SpiController


KB = 1024
MB = 1024 * KB

SOC_DIR = os.path.dirname(os.path.realpath(__file__))


class HpsSoC(LiteXSoC):
    """SoC definition for HPS"""

    # Memory layout
    csr_origin = 0xF0000000
    spiflash_region = SoCRegion(0x20000000, 16 * MB, cached=True)
    # The start of the SPI Flash contains the FPGA gateware. Our ROM is after
    # that.
    rom_offset = 2 * MB
    rom_origin = spiflash_region.origin + rom_offset
    rom_region = SoCRegion(
        rom_origin, spiflash_region.size - rom_offset, cached=True, linker=True
    )
    sram_origin = 0x40000000
    sram_region = SoCRegion(sram_origin, 64 * KB, cached=True, linker=True)
    arena_origin = 0x60000000
    arena_region = SoCRegion(arena_origin, 256 * KB, cached=True, linker=True)
    vexriscv_region = SoCRegion(origin=0xF00F0000, size=0x100)

    # These variables are needed by builder.py. Normally they're defined by
    # SoCCore, but we don't inherit from SoCCore.
    integrated_rom_initialized = False
    integrated_rom_size = rom_region.size

    mem_map = {
        "rom": rom_offset,
        "sram": sram_origin,
        "arena": arena_origin,
        "csr": csr_origin,
    }

    cpu_type = "vexriscvhps"

    def __init__(self, *, platform, cpu_cfu=None, variant=None):
        LiteXSoC.__init__(
            self,
            platform=platform,
            sys_clk_freq=platform.sys_clk_freq,
            csr_data_width=32,
        )

        # Clock, Controller, CPU
        self.submodules.crg = platform.create_crg()
        self.add_cpu(
            self.cpu_type,
            variant=variant,
            reset_address=self.rom_origin,
            cfu=cpu_cfu,
        )

        # RAM and arena
        self.setup_ram()
        self.setup_arena()
        self.setup_dynamic_clock_control()
        self.connect_cfu_to_lram()

        # SPI Flash
        self.setup_litespi_flash()

        # ROM (part of SPI Flash)
        self.bus.add_region("rom", self.rom_region)

        # Camera
        self.setup_camera()

        # SPI controller
        self.submodules.mcu_spi = self.create_spi_controller(
            self.platform.request("mcu_spi", 0)
        )
        self.csr.add("mcu_spi")

    def setup_ram(self):
        self.submodules.sram = ClockDomainsRenamer("osc")(
            self.platform.create_ram(32, self.sram_region.size, dual_port=False)
        )
        self.bus.add_slave("sram", self.sram.bus, self.sram_region)

    def setup_arena(self):
        self.submodules.arena = ClockDomainsRenamer("osc")(
            self.platform.create_ram(32, self.arena_region.size, dual_port=True)
        )
        self.bus.add_slave("arena", self.arena.bus, self.arena_region)
        self.add_config("SOC_SEPARATE_ARENA")

    def setup_dynamic_clock_control(self):
        self.submodules.cfu_cpu_clk_ctl = ClockDomainsRenamer("osc")(
            CfuCpuClockCtrl()
        )
        cfu_cen = self.cfu_cpu_clk_ctl.cfu_cen
        cpu_cen = self.cfu_cpu_clk_ctl.cpu_cen
        ctl_cfu_bus = self.cfu_cpu_clk_ctl.cfu_bus
        cpu_cfu_bus = self.cpu.cfu_bus

        self.comb += [
            # Connect dynamic clock control bus to CPU <-> CFU BUS
            ctl_cfu_bus.rsp.valid.eq(cpu_cfu_bus.rsp.valid),
            ctl_cfu_bus.rsp.ready.eq(cpu_cfu_bus.rsp.ready),
            ctl_cfu_bus.cmd.valid.eq(cpu_cfu_bus.cmd.valid),
            ctl_cfu_bus.cmd.ready.eq(cpu_cfu_bus.cmd.ready),
            # Connect system clock to dynamic clock enable
            self.crg.sys_clk_enable.eq(cpu_cen),
        ]

        # Create separate clock for CFU
        clko = ClockSignal("cfu")
        self.clock_domains.cd_cfu = ClockDomain("cfu")
        self.specials += Instance(
            "DCC",
            i_CLKI=ClockSignal("osc"),
            o_CLKO=clko,
            i_CE=cfu_cen,
        )

        # Connect separate clock to CFU, keep reset from oscillator clock
        # domain
        self.cpu.cfu_params.update(i_clk=clko)
        self.cpu.cfu_params.update(i_reset=ResetSignal("osc"))

        # Connect clock enable signals to arena:
        # A port is enabled when CPU is running,
        # B port is enabled when CFU is running.
        self.comb += [
            self.arena.a_clk_ens[0].eq(cpu_cen),
            self.arena.a_clk_ens[1].eq(cpu_cen),
            self.arena.a_clk_ens[2].eq(cpu_cen),
            self.arena.a_clk_ens[3].eq(cpu_cen),
            self.arena.b_clk_ens[0].eq(cfu_cen),
            self.arena.b_clk_ens[1].eq(cfu_cen),
            self.arena.b_clk_ens[2].eq(cfu_cen),
            self.arena.b_clk_ens[3].eq(cfu_cen),
        ]

        # Enable sram when CPU is running
        self.comb += self.sram.a_clk_ens[0].eq(cpu_cen)

    def connect_cfu_to_lram(self):
        # create cfu <-> lram bus
        cfu_lram_bus_layout = [
            ("lram0", [("addr", 14), ("din", 32)]),
            ("lram1", [("addr", 14), ("din", 32)]),
            ("lram2", [("addr", 14), ("din", 32)]),
            ("lram3", [("addr", 14), ("din", 32)]),
        ]
        cfu_lram_bus = Record(cfu_lram_bus_layout)

        # add extra ports to the cfu pinout
        self.cpu.cfu_params.update(
            o_port0_addr=cfu_lram_bus.lram0.addr,
            i_port0_din=cfu_lram_bus.lram0.din,
            o_port1_addr=cfu_lram_bus.lram1.addr,
            i_port1_din=cfu_lram_bus.lram1.din,
            o_port2_addr=cfu_lram_bus.lram2.addr,
            i_port2_din=cfu_lram_bus.lram2.din,
            o_port3_addr=cfu_lram_bus.lram3.addr,
            i_port3_din=cfu_lram_bus.lram3.din,
        )

        # connect them to the lram module
        self.comb += [
            self.arena.b_addrs[0].eq(cfu_lram_bus.lram0.addr),
            self.arena.b_addrs[1].eq(cfu_lram_bus.lram1.addr),
            self.arena.b_addrs[2].eq(cfu_lram_bus.lram2.addr),
            self.arena.b_addrs[3].eq(cfu_lram_bus.lram3.addr),
            cfu_lram_bus.lram0.din.eq(self.arena.b_douts[0]),
            cfu_lram_bus.lram1.din.eq(self.arena.b_douts[1]),
            cfu_lram_bus.lram2.din.eq(self.arena.b_douts[2]),
            cfu_lram_bus.lram3.din.eq(self.arena.b_douts[3]),
        ]

    def setup_litespi_flash(self):
        self.submodules.spiflash_phy = LiteSPIPHY(
            self.platform.request("spiflash4x"),
            GD25LQ128D(Codes.READ_1_1_4),
            default_divisor=0,
            rate="1:1",
        )
        self.submodules.spiflash_mmap = LiteSPI(
            phy=self.spiflash_phy,
            mmap_endianness=self.cpu.endianness,
            with_master=False,
            with_csr=False,
        )
        self.csr.add("spiflash_mmap")
        self.csr.add("spiflash_phy")
        # Our CPU's ibus (instruction bus) is connected directly to the SPI
        # flash rather than via the main bus that is used by the dbus (data
        # bus). This prevents code from being executed from anywhere other than
        # SPI flash. i.e. we can't run code from RAM, which is better from a
        # security perspective.
        dbus_flash_interface = wishbone.Interface(
            data_width=self.bus.data_width, adr_width=self.bus.address_width
        )
        self.submodules.spiflash_arbiter = wishbone.Arbiter(
            masters=[self.cpu.ibus, dbus_flash_interface],
            target=self.spiflash_mmap.bus,
        )
        self.bus.add_slave(
            name="spiflash",
            slave=dbus_flash_interface,
            region=self.spiflash_region,
        )

    def setup_camera(self):
        self.submodules.cam_control = CameraControl(
            pads=self.platform.request("cam_control")
        )
        self.csr.add("cam_control")

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
            region.type = "cached" if region.cached else "io"
            if region.linker:
                region.type += "+linker"
        self.csr_regions = self.csr.regions  # pylint: disable=W0201

        self.configure_rom()

    def configure_rom(self):
        # We don't want to build the LiteX BIOS since we don't use it.
        self.integrated_rom_size = 0
        self.integrated_rom_initialized = True


def hps_soc_args(parser: argparse.ArgumentParser):
    builder_args(parser)
    parser.add_argument(
        "--nextpnr-seed",
        metavar="NUMBER",
        default=4,
        type=int,
        help="Use random seed NUMBER for nextpnr",
    )


def monkey_patch_fake_picolibc():
    """Fakes a pythondata_software_picolibc module.

    pythondata_software_picolibc is only used by LiteX when building BIOS, which
    our configuration does not do. Nevertheless the module is a hard dependency
    of LiteX and must be present to allow a build to proceed.

    By providing this fake we do not need to have the actual module present,
    which saves some complexity in ebuild files.
    """
    name = "pythondata_software_picolibc"
    module = types.ModuleType(name)
    module.data_location = ""
    sys.modules[name] = module


def create_builder(soc, args):
    """Creates builder, with correct defaults."""
    builder = Builder(soc, **builder_argdict(args))
    builder.compile_software = False
    builder.output_dir = args.output_dir
    monkey_patch_fake_picolibc()
    return builder


def parse_metrics(output_filename):
    """Scrapes interesting metrics from the FPGA toolchain output."""
    with open(output_filename, "r", encoding="latin-1") as f:
        output_lines = f.readlines()
    metrics = {}
    # Find the percentage utilisation of some important FPGA resources.
    for resource_type in ["OXIDE_FF", "OXIDE_COMB", "OXIDE_EBR", "LRAM_CORE"]:
        for line in output_lines:
            m = re.search(rf"{resource_type}:.* (\d+)%$", line)
            if m:
                metrics[f"{resource_type}_PERCENT"] = m.group(1)
    # Find the estimated max frequency. Note that the message is printed twice
    # and only the final one matters.
    for line in reversed(output_lines):
        m = re.search(r"Max frequency for clock.*: (\d+\.\d+) MHz ", line)
        if m:
            metrics["fMAX_MHz"] = m.group(1)
            break
    return metrics


def main():
    CPUS["vexriscvhps"] = VexRiscvHps

    parser = argparse.ArgumentParser(description="HPS SoC")
    hps_soc_args(parser)
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="Skip building gateware. This is useful if you just want to "
        + "quickly regenerate SVD files or associated artifacts.",
    )
    parser.add_argument(
        "--parallel-nextpnr",
        action="store_true",
        help="Runs $(nproc) copies of nextpnr in parallel. Used to search "
        + "for a seed.",
    )

    args = parser.parse_args()
    args.output_dir = "build/hps_platform"
    os.makedirs(args.output_dir, exist_ok=True)

    cfu = hps_cfu.make_cfu(specialize_nx=True)
    cfu_verilog = amaranth.back.verilog.convert(
        cfu, name="Cfu", ports=cfu.ports
    )
    cfu_verilog_filename = os.path.join(args.output_dir, "cfu.v")
    with open(cfu_verilog_filename, "w") as f:
        f.write(cfu_verilog)

    # Our custom Vexriscv configuration is not exactly 'full+cfu'
    # but the differences don't matter to Litex.
    variant = "full+cfu"

    soc = HpsSoC(
        platform=Platform(parallel_nextpnr=args.parallel_nextpnr),
        variant=variant,
        cpu_cfu=cfu_verilog_filename,
    )

    cpu_verilog_path = os.path.join(
        SOC_DIR, "../third_party/vexriscv/VexRiscv_SlimCfu.v"
    )

    soc.cpu.use_external_variant(cpu_verilog_path)

    args.csr_svd = f"{args.output_dir}/hps.svd"
    args.memory_x = f"{args.output_dir}/memory.x"

    builder = create_builder(soc, args)
    builder_kwargs = {
        "abc9": True,
        "timingstrict": True,
        "seed": args.nextpnr_seed,
    }

    print(f"Building with seed {args.nextpnr_seed}")
    start_time = time.time()
    vns = builder.build(**builder_kwargs, run=not args.no_build)
    run_time = time.time() - start_time
    output_log = f"{builder.output_dir}/gateware/hps_platform_output.log"
    metrics = parse_metrics(output_log)
    meta_file = f"{builder.output_dir}/gateware/hps_platform_build.metadata"
    print(f"Execution time for the build is: {run_time}s")
    with open(meta_file, "w", encoding="latin-1") as f:
        f.write(f"RUN_TIME: {run_time}\n")
        for name, value in metrics.items():
            f.write(f"{name}: {value}\n")

    soc.do_exit(vns)

    # Generate the peripheral access crate (PAC) containing Rust code to use the
    # features of our SOC.
    pac_out_dir = f"{builder.output_dir}/litex_pac"
    os.makedirs(pac_out_dir, exist_ok=True)
    subprocess.check_call(
        [
            "svd2rust",
            "--target",
            "riscv",
            "-i",
            f"{builder.output_dir}/hps.svd",
            "--output-dir",
            pac_out_dir,
        ]
    )
    subprocess.check_call(
        ["rustfmt", f"{pac_out_dir}/lib.rs", f"{pac_out_dir}/build.rs"]
    )

    print(f"Build outputs at {builder.output_dir}")


class VexRiscvHps(VexRiscv):
    """The soft CPU used by HPS.

    Differs from VexRiscv in that the ibus (instruction bus) isn't listed as a
    peripheral bus. We handle the ibus separately.
    """

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.periph_buses = [self.dbus]


if __name__ == "__main__":
    main()
