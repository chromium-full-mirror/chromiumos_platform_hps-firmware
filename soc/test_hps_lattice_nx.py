# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Unit tests for third_party/python/CFU-Playground/hps_lattice_nx.py"""

from unittest import TestCase

from hps_lattice_nx import GatedPassThrough
from migen import If
from migen import Module
from migen import run_simulation
from migen import Signal


class GatedDelay(Module):
    """Delays a signal by one cycle with gated updates.

    When clk_en is true, data_out takes the value of data_in from the previous
    cycle. The behavior should be the same whether use_gated_pass_through is
    set, only the implementation differs.
    """

    def __init__(self, use_gated_pass_through):
        self.data_in = Signal(32)
        self.clk_en = Signal()
        self.data_out = Signal(32)

        if use_gated_pass_through:
            self.submodules.pass_through = GatedPassThrough()
            self.sync += self.pass_through.data_in.eq(self.data_in)
            self.comb += self.data_out.eq(self.pass_through.data_out)
            self.comb += self.pass_through.clk_en.eq(self.clk_en)
        else:
            self.sync += If(self.clk_en, self.data_out.eq(self.data_in))


class GatedPassThroughTest(TestCase):
    """Integration test for GatedPassThrough"""

    def setUp(self):
        self.test_module = Module()
        self.duts = [GatedDelay(True), GatedDelay(False)]
        self.test_module.submodules += [self.duts[0], self.duts[1]]

    def test_gated_pass_through(self):
        DATA = [
            (1, True, None),
            (2, True, None),
            (3, True, 1),
            (4, True, 2),
            (5, False, 3),
            (6, False, 4),
            (7, True, 4),
            (8, True, 4),
            (9, True, 7),
        ]

        def run():
            for impl in range(2):
                dut = self.duts[impl]
                for (data_in, clk_en, expected) in DATA:
                    yield dut.data_in.eq(data_in)
                    yield dut.clk_en.eq(clk_en)
                    if expected is not None:
                        actual = yield dut.data_out
                        self.assertEqual(
                            actual,
                            expected,
                            f"dut[{impl}] failed when data_in={data_in}",
                        )
                    yield

        run_simulation(self.test_module, generators=[run()])
