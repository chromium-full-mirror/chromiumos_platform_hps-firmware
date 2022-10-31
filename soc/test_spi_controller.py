# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Unit tests for spi_controller module."""

from itertools import count
import random
from unittest import TestCase

from migen import Module
from migen import passive
from migen import run_simulation
from migen import Signal

from .spi_controller import SpiController


class FakePads:
    """Fake pads for testing SpiController."""

    def __init__(self):
        self.cs = Signal()
        self.clk = Signal()
        self.copi = Signal()
        self.cipo = Signal()
        self.ready = Signal()


def to_bits(vals):
    """Converts a list of 8 bit values to an MSB first bit string"""

    bit_string = "".join(f"{v:08b}" for v in vals)
    return [int(b) for b in bit_string]


class SpiControllerTest(TestCase):
    """Tests for SPI Controller class."""

    # The migen API conventions encourage naming CSRs with leading "_"s, but we
    # need to drive them in this test case
    # pylint: disable=protected-access

    def setUp(self):
        self.pads = FakePads()
        self.dut = SpiController(
            self.pads, sys_clk_freq=100, spi_clk_freq=10, loopback=False
        )
        self.test_module = Module()
        self.test_module.submodules.dut = self.dut
        self.test_module.submodules += self.dut._cr

    def test_cs(self):
        # Toggle CS and ensure its value is reflected at the output pin
        def run():

            # Enable
            yield self.dut._cr.fields.en.eq(1)
            for _ in range(5):
                yield

            # Set CS to 0, wait at most 100 cycles
            yield self.dut._cr.fields.cs.eq(0)
            for _ in range(100):
                yield
                if (yield self.pads.cs) == 0:
                    break
            else:
                self.fail()

            # Set CS to 1, wait at most 100 cycles
            yield self.dut._cr.fields.cs.eq(1)
            for _ in range(100):
                yield
                if (yield self.pads.cs) == 1:
                    break
            else:
                self.fail()

            # Set CS to 0, wait at most 100 cycles
            yield self.dut._cr.fields.cs.eq(0)
            for _ in range(100):
                yield
                if (yield self.pads.cs) == 0:
                    break
            else:
                self.fail()

            for _ in range(5):
                yield

        run_simulation(self.dut, generators=[run()])

    def test_tx_rx(self):
        # Transmit and receive 32 bytes
        random.seed(42)
        data_out = [random.randint(0, 0xFF) for _ in range(32)]
        data_in = [random.randint(0, 0xFF) for _ in range(32)]

        def run():
            # Set enable, then start putting data_out in TXD and checking
            # data_in in RXD
            yield self.dut._cr.fields.en.eq(1)
            yield
            for n, bit_out, expected in zip(count(), data_out, data_in):
                # Wait for transmit empty, then transmit
                while not (yield self.dut._flags.fields.txe):
                    yield
                yield from self.dut._txd.write(bit_out)
                # Wait for receive not empty, then read
                while (yield self.dut._flags.fields.rxe):
                    yield
                bit_in = yield from self.dut._rxd.read()
                self.assertEqual(
                    bit_in, expected, f"Failed data in on byte {n}"
                )

        def wait_for_edge(expected_last, expected_this):
            last_clk = expected_this
            this_clk = yield self.pads.clk
            while last_clk != expected_last or this_clk != expected_this:
                last_clk = this_clk
                yield
                this_clk = yield self.pads.clk

        @passive
        def check_copi():
            # sample on middle edge
            for n, b in enumerate(to_bits(data_out)):
                yield from wait_for_edge(0, 1)
                self.assertEqual(
                    (yield self.pads.copi), b, f"Failed copi on bit {n}"
                )

        @passive
        def send_cipo():
            # place data to be read at start of clock
            for b in to_bits(data_in):
                yield self.pads.cipo.eq(b)
                yield from wait_for_edge(1, 0)

        run_simulation(self.dut, generators=[run(), check_copi(), send_cipo()])
