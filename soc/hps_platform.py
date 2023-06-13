# Copyright 2020 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Platform definition file for HPS proto2"""

from hps_lattice_nx import NXLRAM
from litex.build.generic_platform import IOStandard
from litex.build.generic_platform import Misc
from litex.build.generic_platform import Pins
from litex.build.generic_platform import Subsignal
from litex.build.lattice import LatticePlatform
from litex.build.lattice import oxide
from litex.soc.cores.clock import NXOSCA
from migen import ClockDomain
from migen import ClockSignal
from migen import If
from migen import Instance
from migen import log2_int
from migen import Module
from migen import Signal
from migen.genlib.resetsync import AsyncResetSynchronizer


hps_io = [
    # HM01B0 camera connection
    (
        "cam_control",
        0,
        Subsignal("data", Pins("G8 F7 F8 E7 E8 G9 F9 E9")),
        Subsignal("fvld", Pins("F6")),
        Subsignal("lvld", Pins("G5")),
        Subsignal("pclk", Pins("G7")),
        # Subsignal('int',  Pins('E2')),  # Shared with serial rx
        IOStandard("LVCMOS18H"),
        Misc("SLEWRATE=FAST"),
    ),
    ("programn", 0, Pins("A4"), IOStandard("LVCMOS18H")),
    # JTAG: not usually programatically accessible
    (
        "jtag",
        0,
        Subsignal("en", Pins("C2")),
        Subsignal("tck", Pins("D2")),
        Subsignal("tdi", Pins("C3")),
        Subsignal("tdo", Pins("D3")),
        Subsignal("tms", Pins("B1")),
        IOStandard("LVCMOS18H"),
        Misc("SLEWRATE=FAST"),
    ),
    # SPI flash, defined two ways
    (
        "spiflash",
        0,
        Subsignal("cs_n", Pins("A3")),
        Subsignal("clk", Pins("B4")),
        Subsignal("mosi", Pins("B5")),
        Subsignal("miso", Pins("C4")),
        Subsignal("wp", Pins("B3")),
        Subsignal("hold", Pins("B2")),
        IOStandard("LVCMOS18"),
        Misc("SLEWRATE=FAST"),
    ),
    (
        "spiflash4x",
        0,
        Subsignal("cs_n", Pins("A3")),
        Subsignal("clk", Pins("B4")),
        Subsignal("dq", Pins("B5 C4 B3 B2")),
        IOStandard("LVCMOS18"),
        Misc("SLEWRATE=FAST"),
    ),
    (
        "mcu_spi",
        0,
        Subsignal("clk", Pins("H1")),
        Subsignal("cs", Pins("F2")),
        Subsignal("cipo", Pins("G2")),
        Subsignal("copi", Pins("H2")),
        Subsignal("ready", Pins("F3")),
        IOStandard("LVCMOS18H"),
        Misc("SLEWRATE=FAST"),
    ),
]

# Debug IO that is specific to the HPS hardware. These should have equivalents
# defined in simulation.py if they are referenced from C code.
hps_nx17_debug_io = [
    # Debug UART
    (
        "serial",
        0,
        Subsignal("rx", Pins("E2"), IOStandard("LVCMOS18")),
        Subsignal("tx", Pins("G1"), IOStandard("LVCMOS18H")),
    ),
]

# Debug IO that is common to both simulation and hardware.
hps_debug_common = [("user_led", 0, Pins("G3"), IOStandard("LVCMOS18H"))]


class _CRG(Module):
    """Clock Reset Generator"""

    def __init__(self, platform, sys_clk_freq):
        self.clock_domains.cd_osc = ClockDomain()
        self.clock_domains.cd_sys = ClockDomain()
        self.clock_domains.cd_por = ClockDomain()
        self.sys_clk_enable = Signal(reset=1)

        # Clock from HFOSC
        self.submodules.osc_clk = sys_osc = NXOSCA()
        sys_osc.create_hf_clk(self.cd_osc, sys_clk_freq)
        self.specials += Instance(
            "DCC",
            i_CLKI=ClockSignal("osc"),
            o_CLKO=ClockSignal("sys"),
            i_CE=self.sys_clk_enable,
        )
        self.comb += self.cd_sys.rst.eq(self.cd_osc.rst)
        # We make the period constraint 7% tighter than our actual system
        # clock frequency, because the CrossLink-NX internal oscillator runs
        # at ±7% of nominal frequency.
        platform.add_period_constraint(
            self.cd_osc.clk, 1e9 / (sys_clk_freq * 1.07)
        )

        # Power On Reset
        por_cycles = 4096
        por_counter = Signal(log2_int(por_cycles), reset=por_cycles - 1)
        self.comb += self.cd_por.clk.eq(self.cd_osc.clk)
        self.sync.por += If(por_counter != 0, por_counter.eq(por_counter - 1))
        self.specials += AsyncResetSynchronizer(self.cd_osc, (por_counter != 0))


# Usual template for the bitstream build script.
_standard_build_template = [
    "set -o pipefail",
    "{{",
    "yosys -l {build_name}.rpt {build_name}.ys",
    (
        # Keep a timeout on this, since there's no obvious way to control
        # timing internally. The code that invokes this template provides no
        # way to pass `timeout=` as a param, so unfortunately, this is the best
        # that can be done.
        "timeout --foreground --kill-after=5s --signal=TERM 90m "
        "nextpnr-nexus "
        "--json {build_name}.json "
        "--pdc {build_name}.pdc "
        "--fasm {build_name}.fasm "
        "--device {device} "
        "{timefailarg} "
        "{ignoreloops} "
        "--seed {seed} "
        "--placer-heap-timingweight 50 "
        "--estimate-delay-mult 25 "
    ),
    "prjoxide pack {build_name}.fasm {build_name}.bit",
    "}} 2>&1 | tee {build_name}_output.log",
]


# Parallel build template for use when searching for a seed
_parallel_nextpnr_template = [
    "set -o pipefail",
    "{{",
    "yosys -l {build_name}.rpt {build_name}.ys",
    (
        "parallel-nextpnr-nexus {build_name}.json {build_name}.pdc "
        "{build_name}.fasm  $(nproc) {seed}"
    ),
    "prjoxide pack {build_name}.fasm {build_name}.bit",
    "}} 2>&1 | tee {build_name}_output.log",
]


# Custom yosys script template
_yosys_template = oxide._yosys_template + [  # pylint: disable=protected-access
    "techmap -map +/nexus/cells_sim.v t:VLO t:VHI %u",
    "plugin -i dsp-ff",
    "dsp_ff -rules +/nexus/dsp_rules.txt",
    "hilomap -singleton -hicell VHI Z -locell VLO Z",
    "write_json {build_name}.json",
]


# pylint: disable=abstract-method
class Platform(LatticePlatform):
    """The HPS Platform"""

    # The NX-17 has a 450 MHz clock. Our system clock should be a divisor of
    # that.
    clk_divisor = 5
    sys_clk_freq = int(450e6 / clk_divisor)

    def __init__(self, *, parallel_nextpnr=False):
        # The HPS actually has the LIFCL-17-7UWG72C, but that doesn't seem to
        # be available in Radiant 2.2, at least on Linux.
        device = "LIFCL-17-8UWG72C"
        ios = hps_io + hps_nx17_debug_io + hps_debug_common
        LatticePlatform.__init__(
            self, device=device, io=ios, connectors=[], toolchain="oxide"
        )
        template = (
            _parallel_nextpnr_template
            if parallel_nextpnr
            else _standard_build_template
        )
        self.toolchain.build_template = template
        self.toolchain.yosys_template = _yosys_template

    def create_crg(self):
        return _CRG(self, self.sys_clk_freq)

    def create_ram(self, width, size, *, dual_port):
        return NXLRAM(width, size, dual_port)

    # TODO: add create_programmer function
