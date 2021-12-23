# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""SPI Controller gateware for communication with the MCU."""

from itertools import count
from unittest import TestCase
import random

from migen import Cat, If, Module, Signal, passive, run_simulation

from litex.soc.interconnect.csr import AutoCSR, CSRField, CSRStatus, CSRStorage


# SPI clock
# TODO(avg): Tune upwards when appropriate
DEFAULT_SPI_CLK_FREQ = 750_000


def bitfield(name, size, reset, desc):
    return CSRField(name=name, size=size, reset=reset, description=desc)


class SpiController(Module, AutoCSR):
    """A SPI Controller.

    In HPS, used to exchange messages with the MCU. Uses CPOL, CPHA = 0, 0:
    clock is initially low. Data clocked at low->high transition.
    """

    def __init__(self, pads, sys_clk_freq,
                 spi_clk_freq=DEFAULT_SPI_CLK_FREQ, loopback=False):
        self.sys_clk_freq = sys_clk_freq
        self.loopback = loopback
        self._cr = CSRStorage(
            fields=[
                bitfield('cs', 1, 1, 'CS line (bring low to select)'),
                bitfield('en', 1, 0, 'Enable SPI')],
            name='cr', description='SPI control register')
        self._flags = CSRStatus(
            fields=[
                bitfield('txe', 1, 1, 'TX buffer empty'),
                bitfield('rxe', 1, 1, 'RX buffer empty'),
                bitfield('ready', 1, 1, 'Peripheral ready'),
            ],
            name='flags', description='SPI Status flags'
        )
        self._txd = CSRStorage(
            name='txd',
            size=8,
            description='Transmit data register')
        self._rxd = CSRStatus(
            name='rxd',
            size=8,
            description='Receive data register')
        self.pads = pads

        self._connect_cs()
        self._watch_tx_rx()

        # Add clocking logic
        self.clock_counter_max = int(self.sys_clk_freq // spi_clk_freq) - 1
        self.clock_counter_half = int(self.sys_clk_freq // (spi_clk_freq * 2))
        self.clock_counter = Signal(max=self.clock_counter_max)
        self._run_clock()

        # Add state machine and supporting counters
        self.bit_count = Signal(max=7)
        self.shift_register = Signal(9)
        self._handle_idle()
        self._handle_running()

        # Finally, when not enabled, hold everything in reset
        self._reset_on_disable()

        self._connect_ready()

    def _connect_cs(self):
        """Connect chip select - independent of all other state"""
        self.comb += self.pads.cs.eq(self._cr.fields.cs)

    def _connect_ready(self):
        """Connect the peripheral ready line - independent of all other state"""
        self.comb += self._flags.fields.ready.eq(self.pads.ready)

    def _watch_tx_rx(self):
        """Watch accesses txd and rxd and set txe and rxe accordingly.

        When the receive data buffer, RXD, is read by the CPU (i.e written to
        the bus) then we set the Receive Empty bit (RXE) since the value in the
        buffer has been processed.

        Conversely, when the transmit data buffer is written by the CPU (i.e
        read from the bus) then we clear the Transmit Empty bit (TXE) since
        the buffer is now full.
        """
        self.sync += If(self._rxd.we, self._flags.fields.rxe.eq(1))
        self.sync += If(self._txd.re, self._flags.fields.txe.eq(0))

    def _run_clock(self):
        """Outputs the SPI clock."""
        # Clock counter counts down until it reaches zero
        self.sync += If(self.clock_counter != 0,
                        self.clock_counter.eq(self.clock_counter - 1))
        # Run SPI clock - cycles 0 in first half of count and 1 in second half,
        # returns to zero when done
        second_half = (self.clock_counter <= self.clock_counter_half)
        self.comb += self.pads.clk.eq(second_half & (self.clock_counter != 0))

    def _handle_idle(self):
        """Handle SPI enabled, but idle"""
        # If data placed into TXD move it to shift register and start working
        is_idle = (self.bit_count == 0) & (self.clock_counter == 0)
        self.sync += If(is_idle & (self._flags.fields.txe == 0),
                        self.shift_register[1:].eq(self._txd.storage),
                        self._flags.fields.txe.eq(1),
                        self.bit_count.eq(7),
                        self.clock_counter.eq(self.clock_counter_max))

    def _handle_running(self):
        """Handle SPI enabled and running state"""
        # Always output MSB of shift register
        self.comb += self.pads.copi.eq(self.shift_register[-1])

        # On half clock counter, handle receiving data
        # On last bit, move data to RXD, else put into shift register
        half_counter = self.clock_counter == self.clock_counter_half
        data_in = self.pads.copi if self.loopback else self.pads.cipo
        self.sync += If(half_counter & (self.bit_count == 0),
                        self._rxd.status.eq(
                            Cat(data_in, self.shift_register[1:])),
                        self._flags.fields.rxe.eq(0))
        self.sync += If(half_counter & (self.bit_count != 0),
                        self.shift_register[0].eq(data_in))

        # On zero clock counter but not last bit, shift shift register
        self.sync += If((self.clock_counter == 0) & (self.bit_count != 0),
                        self.bit_count.eq(self.bit_count - 1),
                        self.clock_counter.eq(self.clock_counter_max),
                        self.shift_register[1:].eq(self.shift_register[:-1]))

    def _reset_on_disable(self):
        """If disabled, then hold everything in reset"""
        self.sync += If(~self._cr.fields.en,
                        self.bit_count.eq(0),
                        self.clock_counter.eq(0),
                        self._flags.fields.txe.eq(1),
                        self._flags.fields.rxe.eq(1))


class DummyPads:
    """Dummy pads for testing SpiController."""

    def __init__(self):
        self.cs = Signal()
        self.clk = Signal()
        self.copi = Signal()
        self.cipo = Signal()
        self.ready = Signal()


def to_bits(vals):
    """Converts a list of 8 bit values to an MSB first bit string"""

    bit_string = ''.join(f'{v:08b}' for v in vals)
    return [int(b) for b in bit_string]


class SpiControllerTest(TestCase):
    """Tests for SPI Controller class."""
    # The migen API conventions encourage naming CSRs with leading "_"s, but we
    # need to drive them in this test case
    # pylint: disable=protected-access

    def setUp(self):
        self.pads = DummyPads()
        self.dut = SpiController(
            self.pads,
            sys_clk_freq=100,
            spi_clk_freq=10,
            loopback=False)
        self.test_module = Module()
        self.test_module.submodules.dut = self.dut
        self.test_module.submodules += self.dut._cr

    def test_cs(self):
        # Toggle CS and ensure its value is reflected at the output pin
        def run():
            yield self.dut._cr.fields.cs.eq(1)
            yield
            for _ in range(5):
                self.assertTrue((yield self.pads.cs))
                yield
            yield self.dut._cr.fields.cs.eq(0)
            yield
            for _ in range(25):
                self.assertFalse((yield self.pads.cs))
                yield
            yield self.dut._cr.fields.cs.eq(1)
            yield
            for _ in range(5):
                self.assertTrue((yield self.pads.cs))
                yield
        run_simulation(self.dut, generators=[run()])

    def test_tx_rx(self):
        # Transmit and receive 32 bytes
        random.seed(42)
        data_out = [random.randint(0, 0xff) for _ in range(32)]
        data_in = [random.randint(0, 0xff) for _ in range(32)]

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
                    bit_in, expected, f'Failed data in on byte {n}')

        def wait_for_edge(expected_last, expected_this):
            last_clk = expected_this
            this_clk = (yield self.pads.clk)
            while last_clk != expected_last or this_clk != expected_this:
                last_clk = this_clk
                yield
                this_clk = (yield self.pads.clk)

        @ passive
        def check_copi():
            # sample on middle edge
            for n, b in enumerate(to_bits(data_out)):
                yield from wait_for_edge(0, 1)
                self.assertEqual((yield self.pads.copi), b,
                                 f'Failed copi on bit {n}')

        @ passive
        def send_cipo():
            # place data to be read at start of clock
            for b in to_bits(data_in):
                yield self.pads.cipo.eq(b)
                yield from wait_for_edge(1, 0)

        run_simulation(self.dut, generators=[run(), check_copi(), send_cipo()])
