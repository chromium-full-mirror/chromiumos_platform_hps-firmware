#!/usr/bin/env python3
# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Simulates the HPS SoC on the local machine.

import argparse

from litex.build.sim import SimPlatform
from litex.build.sim.config import SimConfig
from litex.build.io import CRG
from litex.build.generic_platform import Pins, Subsignal
from litex.soc.cores.cpu import CPUS
from litex.soc.integration.common import get_mem_data
from litex.soc.integration.soc_sdram import soc_sdram_argdict
from litex.soc.integration.builder import builder_args, builder_argdict, Builder
from litex.soc.integration.soc_sdram import soc_sdram_args
from litex.soc.cores.bitbang import I2CMasterSim

from hps_soc import RAM_SIZE, hps_soc_args, create_builder, HpsSoC
from hps_platform import hps_io, hps_debug_common

_sim_io = [
    ("sys_clk", 0, Pins(1)),
    ("sys_rst", 0, Pins(1)),
    ("serial", 0,
        Subsignal("source_valid", Pins(1)),
        Subsignal("source_ready", Pins(1)),
        Subsignal("source_data", Pins(8)),

        Subsignal("sink_valid", Pins(1)),
        Subsignal("sink_ready", Pins(1)),
        Subsignal("sink_data", Pins(8)),
     ),
    # Connection to Camera. This differs from the nx17 version in that sda is
    # split into `in` and `out` subsignals in order to avoid a tristate.
    ("i2c", 1,
        Subsignal("scl", Pins(1)),
        Subsignal("sda_in", Pins(1)),
        Subsignal("sda_out", Pins(1)),
     ),
]


class HpsSimPlatform(SimPlatform):
    sys_clk_freq = 1e6

    def __init__(self):
        SimPlatform.__init__(self, "SIM", hps_io + _sim_io + hps_debug_common)

    def create_crg(self):
        return CRG(self.request("sys_clk"))


class HpsSimSoC(HpsSoC):
    def __init__(self, platform):
        super().__init__(platform=platform, debug=False)
        # we must always request the sim_trace signal
        self.comb += platform.trace.eq(1)

    def setup_ram(self):
        self.add_ram("sram", self.mem_map["sram"], self.sram_region.size)

    def setup_flash(self):
        self.add_rom("spiflash", self.spiflash_region.origin,
                     self.spiflash_region.size, [], "r")

    def add_serial(self):
        self.add_uart("sim")

    def initialize_rom(self, data):
        self.spiflash.mem.init = [0] * int(self.rom_offset / 4) + data

    def configure_rom(self):
        pass

    def create_i2c_controller(self, pads):
        return I2CMasterSim(pads)


def main():
    cpu = CPUS["vexriscv"]
    parser = argparse.ArgumentParser(description="HPS SoC simulation")
    hps_soc_args(parser)
    soc_sdram_args(parser)
    parser.add_argument("--trace", action="store_true",
                        help="Whether to enable tracing of simulation")
    args = parser.parse_args()

    platform = HpsSimPlatform()
    soc = HpsSimSoC(platform=platform)
    sim_config = SimConfig()
    sim_config.add_clocker("sys_clk", freq_hz=soc.sys_clk_freq)
    sim_config.add_module("serial2console", "serial")

    builder = create_builder(soc, args)
    builder.build(
        build=True,
        run=True,
        sim_config=sim_config,
        trace=args.trace,
        trace_fst=True)


if __name__ == "__main__":
    main()
