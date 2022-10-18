# Copyright 2021 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""SPI Controller gateware for communication with the MCU."""

from litex.soc.interconnect.csr import AutoCSR
from litex.soc.interconnect.csr import CSRField
from litex.soc.interconnect.csr import CSRStatus
from litex.soc.interconnect.csr import CSRStorage
from migen import Cat
from migen import If
from migen import Module
from migen import Signal


# With the MCU's APB clock running at 12MHz, 1.5MHz is the fastest SPI clock
# that it can currently keep up with.
DEFAULT_SPI_CLK_FREQ = 1_500_000


def bitfield(name, size, reset, desc):
    return CSRField(name=name, size=size, reset=reset, description=desc)


class SpiController(Module, AutoCSR):
    """A SPI Controller.

    In HPS, used to exchange messages with the MCU. Uses CPOL, CPHA = 0, 0:
    clock is initially low. Data clocked at low->high transition.
    """

    def __init__(
        self,
        pads,
        sys_clk_freq,
        spi_clk_freq=DEFAULT_SPI_CLK_FREQ,
        loopback=False,
    ):
        self.sys_clk_freq = sys_clk_freq
        self.loopback = loopback
        self._cr = CSRStorage(
            fields=[
                bitfield("cs", 1, 1, "CS line (bring low to select)"),
                bitfield("en", 1, 0, "Enable SPI"),
            ],
            name="cr",
            description="SPI control register",
        )
        self._flags = CSRStatus(
            fields=[
                bitfield("txe", 1, 1, "TX buffer empty"),
                bitfield("rxe", 1, 1, "RX buffer empty"),
                bitfield("ready", 1, 1, "Peripheral ready"),
            ],
            name="flags",
            description="SPI Status flags",
        )
        self._txd = CSRStorage(
            name="txd", size=8, description="Transmit data register"
        )
        self._rxd = CSRStatus(
            name="rxd", size=8, description="Receive data register"
        )
        self.pads = pads

        # Prescaler
        # Counts from self.ps_top to -1(inclusive) which corresponds to
        # 2x the SPI frequency. The counter is one bit wider as detection
        # of overflow to -1 is less expensive in logic. Signal ps_tick
        # is asserted for one clock cycle each time the prescaler overflows.
        ps_max = int(self.sys_clk_freq // (spi_clk_freq * 2))
        self.ps_top = ps_max - 2
        assert self.ps_top >= 0

        self.ps_cnt = Signal(self.ps_top.bit_length() + 1)
        self.ps_pha = Signal()
        self.ps_tick = Signal()

        # Bit counter and shift register. The bit counter works similarly to
        # the prescaler
        self.bit_count = Signal(4, reset=-1)
        self.shift_register = Signal(8)

        # Busy signal. Set to high when a byte transfer is pending
        self.busy = Signal()
        self.comb += self.busy.eq(~self.bit_count[-1])

        # Run stuff
        self._connect_cs()
        self._connect_ready()
        self._watch_tx_rx()
        self._run_prescaler()
        self._run_clock()
        self._handle_transmission()
        self._reset_on_disable()

    def _connect_cs(self):
        """Connect chip select but synchronize it with CLK.

        Does not allow changing CS during transmission.
        """
        self.sync += If(
            ~self.busy & self.ps_tick, self.pads.cs.eq(self._cr.fields.cs)
        )

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

    def _run_prescaler(self):
        """Prescaler logic"""

        # Counter
        self.sync += If(self.ps_cnt[-1] == 1, self.ps_cnt.eq(self.ps_top)).Else(
            self.ps_cnt.eq(self.ps_cnt - 1)
        )

        # Phase
        self.sync += If(self.ps_cnt[-1] == 1, self.ps_pha.eq(~self.ps_pha))

        # Tick
        self.sync += self.ps_tick.eq(
            ~self.ps_tick & self.ps_cnt[-1] & self.ps_pha
        )

    def _run_clock(self):
        """Outputs the SPI clock."""
        self.sync += self.pads.clk.eq(self.busy & self.ps_pha)

    def _handle_transmission(self):
        """Handles data transmission"""

        # Data input with input filter. The signal from outside the fabric
        # passes through two flip-flops to remove any possible metastablilty.
        din_raw = self.pads.copi if self.loopback else self.pads.cipo
        din_dly = Signal(2)
        din = Signal()

        self.sync += din_dly.eq(Cat(din_raw, din_dly[:-1]))
        self.comb += din.eq(din_dly[-1])

        # TX logic
        self.comb += self.pads.copi.eq(self.shift_register[-1])
        self.sync += If(
            self.ps_tick,
            If(
                ~self.busy & ~self._flags.fields.txe,
                self._flags.fields.txe.eq(1),
                self.bit_count.eq(7),
                self.shift_register.eq(self._txd.storage),
            ).Elif(
                self.busy,
                self.bit_count.eq(self.bit_count - 1),
                self.shift_register[1:].eq(self.shift_register[:-1]),
            ),
        )

        # RX logic
        self.sync += If(
            self.busy & self.ps_tick, self.shift_register[0].eq(din)
        )

        # Output data latch. Detects end of the busy state
        prev = Signal()
        self.sync += prev.eq(~self.busy)
        self.sync += If(
            ~prev & ~self.busy & self._flags.fields.rxe,
            self._rxd.status.eq(self.shift_register),
            self._flags.fields.rxe.eq(0),
        )

    def _reset_on_disable(self):
        """If disabled, then hold everything in reset"""
        self.sync += If(
            ~self._cr.fields.en,
            self.bit_count.eq(-1),
            self.ps_cnt.eq(self.ps_top),
            self._flags.fields.txe.eq(1),
            self._flags.fields.rxe.eq(1),
            self.pads.cs.eq(1),
            self.pads.clk.eq(0),
        )
