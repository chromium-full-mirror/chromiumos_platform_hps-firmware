# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Gateware for receiving data from the HM01B0 camera."""

from migen import If, Module, Mux, Signal, Record

from litex.soc.interconnect import stream
from litex.soc.interconnect.csr import AutoCSR, CSRField, CSRStatus, CSRStorage
from litex.soc.cores import dma


# Depth of FIFO used for the camera data DMA, in 8-bit bytes.
# Note that this is in addition to the words stored elsewhere in the pipeline.
# Specifically, 4 bit bytes are buffered in the converter.
DMA_FIFO_DEPTH = 4


class CameraControl(Module, AutoCSR):
    """Handles all the non-I2C signals for the camera"""

    def __init__(self, pads, sys_clk_freq):
        self.sys_clk_freq = sys_clk_freq
        self._w = CSRStorage(fields=[
            CSRField('dma_signed', size=1, offset=2)],
            name='w')
        self._r = CSRStatus(fields=[
            CSRField('lvld', size=1, offset=0),
            CSRField('fvld', size=1, offset=1),
            CSRField('pclk', size=1, offset=2)],
            name='r')
        self._dat = CSRStatus(fields=[CSRField('data', size=8, offset=0)],
                              name='dat')
        self.buffered = self.cross_domains(pads)
        self.connect_pads(self.buffered)

    def cross_domains(self, pads):
        _LAYOUT = [
            ('lvld', 1),
            ('fvld', 1),
            ('pclk', 1),
            ('data', 8),
        ]

        sigs = Record(_LAYOUT)
        self.comb += [
            sigs.lvld.eq(pads.lvld),
            sigs.fvld.eq(pads.fvld),
            sigs.pclk.eq(pads.pclk),
            sigs.data.eq(pads.data)]
        for _ in range(2):
            sigs_next = Record(_LAYOUT)
            self.sync += sigs_next.eq(sigs)
            sigs = sigs_next
        return sigs

    def connect_pads(self, buffered):
        """Connect all the straight-through signals."""
        self.sync += [
            self._r.fields.lvld.eq(buffered.lvld),
            self._r.fields.fvld.eq(buffered.fvld),
            self._r.fields.pclk.eq(buffered.pclk),
            self._dat.fields.data.eq(buffered.data)
        ]


class CameraControlDMA(CameraControl):
    """Handles camera control and accepting DMA data"""

    def __init__(self, *, bus, pads, sys_clk_freq):
        super().__init__(
            pads=pads,
            sys_clk_freq=sys_clk_freq)
        self.bus = bus
        self.connect_dma(self.buffered)

    def connect_dma(self, buffered):
        """Connect incoming data and clock to DMA."""
        # TODO: more sophisticated reset/begin/end/timeout logic for writer.
        # NOTE: the writer writes 32bit words to the bus, each of which
        # contains four 8bit data packets from the camera.
        self.submodules.dma = dma.WishboneDMAWriter(self.bus, with_csr=True)
        self.submodules.fifo = stream.SyncFIFO([('data', 8)], DMA_FIFO_DEPTH)
        self.submodules.converter = stream.Converter(
            8, self.bus.data_width, reverse=True)
        self.comb += [
            self.fifo.source.connect(self.converter.sink),
            self.converter.source.connect(self.dma.sink),
        ]

        # Detect pixel clock rising edge
        last_pclk = Signal()
        self.sync += last_pclk.eq(buffered.pclk)
        pclk_rising = buffered.pclk & ~last_pclk

        # When dma is enabled, and line clock (lvld) is high, and pclk is at a
        # rising edge, then capture data.
        # NOTE: Ignores the fifo "ready" signal since we are unable to apply
        # back pressure to the camera data interface. Instead, we set the FIFO
        # size large enough to buffer incoming data before it is written to
        # RAM via DMA.
        self.sync += [
            # pylint: disable=protected-access
            If(self.dma._enable.storage & buffered.lvld & pclk_rising, [
                self.fifo.sink.data.eq(buffered.data -
                                       Mux(self._w.fields.dma_signed, 128, 0)),
                self.fifo.sink.valid.eq(1),
            ]).Else([
                self.fifo.sink.valid.eq(0),
            ])
        ]
