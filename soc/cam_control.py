#!/usr/bin/env python3
# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

from migen import *

from litex.soc.interconnect import stream
from litex.soc.interconnect.csr import *
from litex.soc.cores.dma import *


# Camera clock
CAM_MCLK_FREQ = 10e6

# Depth of FIFO used for the camera data DMA, in 8-bit bytes.
# Note that this is in addition to the words stored elsewhere in the pipeline.
# Specifically, 4 bit bytes are buffered in the converter.
DMA_FIFO_DEPTH = 4


class CameraControl(Module, AutoCSR):
    """All non-I2C signals for the camera
    """

    def __init__(self, pads, sys_clk_freq):
        self.sys_clk_freq = sys_clk_freq
        self._w = CSRStorage(fields=[
            CSRField("trig", size=1, offset=1),
            CSRField("mclk_en", size=1, offset=2)],
            name="w")
        self._r = CSRStatus(fields=[
            CSRField("lvld", size=1, offset=0),
            CSRField("fvld", size=1, offset=1),
            CSRField("pclk", size=1, offset=2)],
            name="r")
        self._dat = CSRStatus(fields=[CSRField("data", size=8, offset=0)],
                              name="dat")
        self.connect_mclk(pads)
        self.connect_pads(pads)

    def connect_mclk(self, pads):
        """Handle the MCLK signal
        """
        # Only change enabled state while clock is low - prevents
        # short clock pulses.
        clk = Signal()
        enabled = Signal()
        self.sync += If(~clk, enabled.eq(self._w.fields.mclk_en))

        # Clock divider - always running
        # TODO: Use a PLL to generate clock
        # Divide sys_clk by constant to get master clock
        div = int(self.sys_clk_freq / CAM_MCLK_FREQ)
        count = Signal((div - 1).bit_length())
        self.sync += clk.eq(count < (div // 2))
        self.sync += If(count == 0, count.eq(div - 1)
                        ).Else(count.eq(count - 1))
        # output clock only when enabled.
        self.sync += pads.mclk.eq(enabled & clk)

    def connect_pads(self, pads):
        """Connect all the straight-through signals.
        """
        self.sync += [
            pads.trig.eq(self._w.fields.trig),
        ]
        self.sync += [
            self._r.fields.lvld.eq(pads.lvld),
            self._r.fields.fvld.eq(pads.fvld),
            self._r.fields.pclk.eq(pads.pclk),
        ]
        self.sync += [
            self._dat.fields.data.eq(pads.data)
        ]


class CameraControlDMA(CameraControl):
    """Handles camera control and accepting DMA data"""

    def __init__(self, *, bus, pads, sys_clk_freq):
        super().__init__(
            pads=pads,
            sys_clk_freq=sys_clk_freq)
        self.bus = bus
        self.connect_dma(pads)

    def connect_dma(self, pads):
        """Connect incoming data and clock to DMA."""
        # TODO: more sophisticated reset/begin/end/timeout logic for writer.
        # NOTE: the writer writes 32bit words to the bus, each of which
        # contains four 8bit data packets from the camera.
        self.submodules.dma = WishboneDMAWriter(self.bus, with_csr=True)
        self.submodules.fifo = stream.SyncFIFO([("data", 8)], DMA_FIFO_DEPTH)
        self.submodules.converter = stream.Converter(
            8, self.bus.data_width, reverse=True)
        self.comb += [
            self.fifo.source.connect(self.converter.sink),
            self.converter.source.connect(self.dma.sink),
        ]

        # Detect pixel clock rising edge
        last_pclk = Signal()
        self.sync += last_pclk.eq(pads.pclk)
        pclk_rising = pads.pclk & ~last_pclk

        # When dma is enabled, and line clock (lvld) is high, and pclk is at a
        # rising edge, then capture data.
        # NOTE: Ignores the fifo "ready" signal since we are unable to apply
        # back pressure to the camera data interface. Instead, we set the FIFO
        # size large enough to buffer incoming data before it is written to
        # RAM via DMA.
        self.sync += [
            If(self.dma._enable.storage & pads.lvld & pclk_rising, [
                self.fifo.sink.data.eq(pads.data),
                self.fifo.sink.valid.eq(1),
            ]).Else([
                self.fifo.sink.valid.eq(0),
            ])
        ]
