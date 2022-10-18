# Copyright 2021 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Gateware for receiving data from the HM01B0 camera."""

from litex.soc.interconnect import stream
from litex.soc.interconnect.csr import AutoCSR
from litex.soc.interconnect.csr import CSRStatus
from litex.soc.interconnect.csr import CSRStorage
from migen import Cat
from migen import FSM
from migen import If
from migen import Module
from migen import NextState
from migen import NextValue
from migen import Record
from migen import ResetInserter
from migen import Signal


# Defines the I/O input signals from the camera
PAD_LAYOUT = [
    ("lvld", 1),
    ("fvld", 1),
    ("pclk", 1),
    ("data", 8),
]

# Layouts for various streams
_PIXEL_STREAM_LAYOUT = stream.EndpointDescription(
    [
        ("data", 8),  # The pixel value
    ]
)
_WORD_STREAM_LAYOUT = stream.EndpointDescription([("data", 32)])


class PixelProducer(Module):
    """Receives pixels from camera puts them on a stream.

    Does not respect backpressure from the output stream.
    """

    def __init__(self, *, pads):
        self.output = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        sigs = Record(PAD_LAYOUT)
        self.comb += sigs.eq(self._cross_domain(pads))
        self.start_of_row = Signal()
        self.start_of_frame = Signal()

        # Find rising edge of pixel clock
        last_pclk = Signal()
        self.sync += last_pclk.eq(sigs.pclk)
        pclk_rising = sigs.pclk & ~last_pclk

        # Find rising edge of line clock
        last_lvld = Signal()
        self.sync += last_lvld.eq(sigs.lvld)
        self.start_of_row = sigs.lvld & ~last_lvld

        # Find rising edge of frame clock
        last_fvld = Signal()
        self.sync += last_fvld.eq(sigs.fvld)
        self.start_of_frame = sigs.fvld & ~last_fvld

        # On pclk rising, while frame valid
        # Send item on output stream
        self.sync += If(
            sigs.fvld & pclk_rising,
            self.output.valid.eq(1),
            self.output.data.eq(sigs.data),
        )

        # Lower valid after a transfer
        self.sync += If(
            self.output.ready & self.output.valid, self.output.valid.eq(0)
        )

    def _cross_domain(self, pads):
        """Bring signals save across from I/O Domain"""
        cross = Record(PAD_LAYOUT)
        self.comb += [
            cross.lvld.eq(pads.lvld),
            cross.fvld.eq(pads.fvld),
            cross.pclk.eq(pads.pclk),
            cross.data.eq(pads.data),
        ]
        for _ in range(2):
            cross_next = Record(PAD_LAYOUT)
            self.sync += cross_next.eq(cross)
            cross = cross_next
        return cross


class Add128Pipeline(stream.PipelinedActor):
    """Adds 128 to incoming data.

    This has the effect of converting the unsigned 0-255 representation
    to a signed -128-127 representation.
    """

    def __init__(self):
        # sink is input, source is output
        self.sink = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.source = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        super().__init__(latency=1)

        self.sync += self.source.data.eq(self.sink.data + 128)


class WordPacker(Module):
    """Packs 4 8 bit pixel values into a 32 bit word.

    Similar to stream.Gearbox.
    """

    def __init__(self):
        self.input = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.output = stream.Endpoint(_WORD_STREAM_LAYOUT)
        byte0 = Signal(8)
        byte1 = Signal(8)
        byte2 = Signal(8)
        self.comb += [
            self.output.data.eq(Cat(byte0, byte1, byte2, self.input.data)),
        ]

        self.submodules.fsm = FSM(reset_state="receive0")
        self.fsm.act(
            "receive0",
            self.input.ready.eq(1),
            self.output.valid.eq(0),
            If(
                self.input.valid & self.output.ready,
                NextValue(byte0, self.input.data),
                NextState("receive1"),
            ),
        )
        self.fsm.act(
            "receive1",
            self.input.ready.eq(1),
            self.output.valid.eq(0),
            If(~self.output.ready, NextState("receive0")),
            If(
                self.input.valid,
                NextValue(byte1, self.input.data),
                NextState("receive2"),
            ),
        )
        self.fsm.act(
            "receive2",
            self.input.ready.eq(1),
            self.output.valid.eq(0),
            If(~self.output.ready, NextState("receive0")),
            If(
                self.input.valid,
                NextValue(byte2, self.input.data),
                NextState("receive3"),
            ),
        )
        self.fsm.act(
            "receive3",
            self.input.ready.eq(self.output.ready),
            self.output.valid.eq(self.input.valid),
            If(~self.output.ready, NextState("receive0")),
            If(self.output.valid, NextState("receive0")),
        )


class CameraControl(Module, AutoCSR):
    """Handles outputs from the camera and outputs bytes of data to memory.

    Device has three state:
    - idle: may be started
    - running: Transfer started but not yet finished
    """

    def __init__(self, *, pads):
        self._running = CSRStatus(
            1,
            description="Set when transfer start is requested and reset when "
            + "transfer is complete.",
        )
        self._idle = CSRStatus(
            1, description="Device may be started only when idle."
        )
        self._reset = CSRStorage(
            1,
            description="Write a 1 to this field to force the device "
            + "to reset",
        )
        self._start_run = CSRStorage(
            1,
            description="Writing a 1 to this field while idle starts "
            + "the transfer.",
        )
        self._wait_row = CSRStorage(
            1,
            description="Writing a 1 to this field while running waits "
            + "for the start of the next row.",
        )
        self._pixels_ready = CSRStatus(
            1, description="Whether we have 4 bytes of pixel data ready."
        )
        self._pixels = CSRStatus(32, description="4 bytes of pixel data.")

        # Contruct all modules
        modules_reset = Signal()
        modules = [
            ("producer", PixelProducer(pads=pads)),
            ("adder", Add128Pipeline()),
            ("packer", WordPacker()),
        ]
        for (name, module) in modules:
            wrapped = ResetInserter()(module)
            setattr(self.submodules, name, wrapped)
            self.comb += wrapped.reset.eq(modules_reset)

        # Connect streams between modules
        self.comb += [
            self.producer.output.connect(self.adder.sink),
            self.adder.source.connect(self.packer.input),
        ]

        reset_requested = self._reset.re & self._reset.storage
        next_row_requested = self._wait_row.re & self._wait_row.storage
        self.submodules.fsm = FSM(reset_state="fsm_reset")
        self.fsm.act("fsm_reset", modules_reset.eq(1), NextState("idle"))
        self.fsm.act(
            "idle",
            self._idle.status.eq(True),
            # When requested, start
            If(
                self._start_run.re & self._start_run.storage,
                NextState("waiting_for_frame"),
            ),
            # When requested, reset
            If(reset_requested, NextState("fsm_reset")),
        )
        self.fsm.act(
            "waiting_for_frame",
            self._running.status.eq(True),
            self.packer.output.ready.eq(0),
            self._pixels_ready.status.eq(0),
            If(self.producer.start_of_frame, NextState("waiting_for_row")),
            # When requested, reset
            If(reset_requested, NextState("fsm_reset")),
        )
        self.fsm.act(
            "waiting_for_row",
            self._running.status.eq(True),
            self.packer.output.ready.eq(0),
            self._pixels_ready.status.eq(0),
            If(self.producer.start_of_row, NextState("waiting_for_data")),
            # When requested, reset
            If(reset_requested, NextState("fsm_reset")),
        )
        self.fsm.act(
            "waiting_for_data",
            self._running.status.eq(True),
            self.packer.output.ready.eq(1),
            If(
                self.packer.output.valid,
                NextValue(self._pixels.status, self.packer.output.data),
                self._pixels_ready.status.eq(1),
                NextState("has_data"),
            ),
            If(next_row_requested, NextState("waiting_for_row")),
            # When requested, reset
            If(reset_requested, NextState("fsm_reset")),
        )
        self.fsm.act(
            "has_data",
            self._running.status.eq(True),
            self.packer.output.ready.eq(1),
            self._pixels_ready.status.eq(1),
            # When the CPU reads the pixel data (we write to the bus), clear the
            # pixels_ready bit and return to the waiting state.
            If(
                self._pixels.we,
                self._pixels_ready.status.eq(0),
                NextState("waiting_for_data"),
            ),
            If(next_row_requested, NextState("waiting_for_row")),
            # When requested, reset
            If(reset_requested, NextState("fsm_reset")),
        )
