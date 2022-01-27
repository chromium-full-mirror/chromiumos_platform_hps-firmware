# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Gateware for receiving data from the HM01B0 camera."""

from migen import (
    Cat, FSM, If, Module, Mux, NextState, NextValue,
    Signal, Record, ResetInserter)

from litex.soc.interconnect import stream
from litex.soc.interconnect.csr import AutoCSR, CSRStatus, CSRStorage


# Defines the I/O input signals from the camera
PAD_LAYOUT = [
    ('lvld', 1),
    ('fvld', 1),
    ('pclk', 1),
    ('data', 8),
]

# Layouts for various streams
_PIXEL_STREAM_LAYOUT = stream.EndpointDescription([
    ('data', 8),  # The pixel value
])
_WORD_STREAM_LAYOUT = stream.EndpointDescription([('data', 32)])

# Raw camera image dimensions
# Must match camera configuration - eg 324x244 (QVGA) or 324x324
_RAW_IMAGE_COLUMNS = 324
_RAW_IMAGE_ROWS = 324
_NUM_PIXELS = _RAW_IMAGE_COLUMNS * _RAW_IMAGE_ROWS


class PixelProducer(Module):
    """Receives pixels from camera puts them on a stream.

    Also signals the first and last pixel of a frame.

    Assumes exactly num_pixels per valid frame, but will resynchronize
    on start of frame.

    Does not respect backpressure from the output stream.
    """

    def __init__(self, *, pads, num_pixels=_NUM_PIXELS):
        self.output = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.index = Signal(max=num_pixels)
        sigs = Record(PAD_LAYOUT)
        self.comb += sigs.eq(self._cross_domain(pads))

        # Find rising edge of pixel clock
        last_pclk = Signal()
        self.sync += last_pclk.eq(sigs.pclk)
        pclk_rising = sigs.pclk & ~last_pclk

        # On pclk rising, while frame valid
        # Send item on output stream and increment index
        self.sync += If(sigs.fvld & pclk_rising,
                        self.output.valid.eq(1),
                        self.output.data.eq(sigs.data),
                        self.output.first.eq(self.index == 0),
                        self.output.last.eq(self.index == num_pixels - 1),
                        self.index.eq(self.index + 1),
                        )

        # Lower valid after a transfer
        self.sync += If(self.output.ready & self.output.valid,
                        self.output.valid.eq(0))

        # While frame is not valid, reset pixel index
        self.sync += If(~sigs.fvld,
                        self.index.eq(0))

    def _cross_domain(self, pads):
        """Bring signals save across from I/O Domain"""
        cross = Record(PAD_LAYOUT)
        self.comb += [
            cross.lvld.eq(pads.lvld),
            cross.fvld.eq(pads.fvld),
            cross.pclk.eq(pads.pclk),
            cross.data.eq(pads.data)]
        for _ in range(2):
            cross_next = Record(PAD_LAYOUT)
            self.sync += cross_next.eq(cross)
            cross = cross_next
        return cross


class FrameGate(Module):
    """Allows full frames to pass, on request.

    input: Endpoint() in
      The input pixels

    output: Endpoint() out
      The output pixels

    request: Signal() in
      Pulse high for a single cycle to request that next frame be passed
      through the gate.
    """

    def __init__(self):
        self.input = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.output = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.request = Signal()
        self.last = Signal()
        self._get_requested = Signal()
        self._getting = Signal()
        self._in_frame = Signal()

        # Always ready to receive
        self.comb += self.input.ready.eq(1)

        # The conditions under which getting will start and stop
        start_getting = (
            self.input.valid & self.input.first & self._get_requested)
        self.comb += self.last.eq(self.input.valid & self.input.last)

        # When "getting", comb input to output
        self.comb += If(
            start_getting | self._getting,
            self.output.valid.eq(self.input.valid),
            self.output.first.eq(self.input.first),
            self.output.last.eq(self.input.last),
            self.output.data.eq(self.input.data),
        )

        # Start and stop "getting"
        self.sync += If(
            start_getting,
            self._getting.eq(1),
            self._get_requested.eq(0),
        ).Elif(
            self.last,
            self._getting.eq(0),
        )

        # Set _get_requested when requested
        self.sync += If(self.request, self._get_requested.eq(1))


class Cropper(Module):
    """Crops rectangular regions from a larger 324 x 324 image.

       Does not provide or respect backpressure.
    """

    def __init__(self, raw_columns, raw_rows):
        self.column_start = Signal(max=raw_columns)
        self.column_end = Signal(max=raw_columns)
        self.row_start = Signal(max=raw_rows)
        self.row_end = Signal(max=raw_rows)
        self.input = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.output = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self._curr_column = Signal(max=raw_columns)
        self._curr_row = Signal(max=raw_rows)

        column_in_range = (
            self._curr_column >= self.column_start) & (
            self._curr_column <= self.column_end)

        row_in_range = (
            self._curr_row >= self.row_start) & (
            self._curr_row <= self.row_end)

        in_range = column_in_range & row_in_range

        # Always ready to receive. Applies no back pressure
        self.comb += self.input.ready.eq(1)

        # Plumb data through then set output valid on transfer when
        # in row and column range
        self.comb += [
            self.output.data.eq(self.input.data),
            self.output.valid.eq(self.input.valid & in_range),
            self.output.last.eq((self._curr_column == self.column_end) & (
                self._curr_row == self.row_end))
        ]

        # Increment counters
        at_last_column = self._curr_column == (raw_columns - 1)
        at_last_row = self._curr_row == (raw_rows - 1)
        self.sync += If(self.input.valid,
                        If(at_last_column, self._curr_column.eq(0),
                           If(at_last_row, self._curr_row.eq(0)
                              ).Else(self._curr_row.eq(self._curr_row + 1))
                           ).Else(self._curr_column.eq(self._curr_column + 1)))


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

    Similar to stream.Gearbox. Passes on last signal if last is set on
    last byte of a word.
    """

    def __init__(self):
        self.input = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.output = stream.Endpoint(_WORD_STREAM_LAYOUT)
        byte0 = Signal(8)
        byte1 = Signal(8)
        byte2 = Signal(8)
        self.comb += [
            self.output.data.eq(
                Cat(byte0, byte1, byte2, self.input.data)),
            self.output.last.eq(self.input.last),
        ]

        self.submodules.fsm = FSM(reset_state='receive0')
        self.fsm.act('receive0',
                     self.input.ready.eq(1),
                     self.output.valid.eq(0),
                     If(self.input.valid,
                        NextValue(byte0, self.input.data),
                        NextState('receive1'))
                     )
        self.fsm.act('receive1',
                     self.input.ready.eq(1),
                     self.output.valid.eq(0),
                     If(self.input.valid,
                        NextValue(byte1, self.input.data),
                        NextState('receive2'))
                     )
        self.fsm.act('receive2',
                     self.input.ready.eq(1),
                     self.output.valid.eq(0),
                     If(self.input.valid,
                        NextValue(byte2, self.input.data),
                        NextState('receive3'))
                     )
        self.fsm.act('receive3',
                     self.input.ready.eq(self.output.ready),
                     self.output.valid.eq(self.input.valid),
                     If(self.output.valid & self.output.ready,
                        NextState('receive0'))
                     )


class CameraControl(Module, AutoCSR):
    """Handles outputs from the camera and outputs bytes of data to memory.

    Device has three state:
    - idle: may be started
    - running: Transfer started but not yet finished
    """

    def __init__(self, *, pads, raw_columns=_RAW_IMAGE_COLUMNS,
                 raw_rows=_RAW_IMAGE_ROWS):
        self._running = CSRStatus(
            1,
            description='Set when transfer start is requested and reset when ' +
            'transfer is complete.')
        self._idle = CSRStatus(
            1, description='Device may be started only when idle.')
        self._reset = CSRStorage(
            1,
            description='Write a 1 to this field to force the device ' +
            'to reset')
        self._start_run = CSRStorage(
            1,
            description='Writing a 1 to this field while idle starts the transfer.')
        self._pixels_ready = CSRStatus(
            1, description='Whether we have 4 bytes of pixel data ready.')
        self._pixels = CSRStatus(
            32, description='4 bytes of pixel data.')
        self._column_start = CSRStorage(
            9, description='Index of column on which to start')
        self._column_count = CSRStorage(
            9, description='Number of columns to read')
        self._row_start = CSRStorage(
            9, description='Index of input row on which to start')
        self._row_count = CSRStorage(
            9, description='Number of consecutive rows to read')

        # Contruct all modules
        modules_reset = Signal()
        modules = [
            ('producer', PixelProducer(pads=pads,
                                       num_pixels=raw_columns * raw_rows)),
            ('frame_gate', FrameGate()),
            ('cropper', Cropper(raw_columns, raw_rows)),
            ('adder', Add128Pipeline()),
            ('fifo', stream.SyncFIFO(_PIXEL_STREAM_LAYOUT, depth=4)),
            ('packer', WordPacker()),
        ]
        for (name, module) in modules:
            wrapped = ResetInserter()(module)
            setattr(self.submodules, name, wrapped)
            self.comb += wrapped.reset.eq(modules_reset)

        # Connect streams between modules
        self.comb += [
            self.producer.output.connect(self.frame_gate.input),
            self.frame_gate.output.connect(self.cropper.input),
            self.cropper.output.connect(self.adder.sink),
            self.adder.source.connect(self.fifo.sink),
            self.fifo.source.connect(self.packer.input),
        ]

        column_start = self._column_start.storage
        column_end = self._column_start.storage + self._column_count.storage - 1
        row_start = self._row_start.storage
        row_end = self._row_start.storage + self._row_count.storage - 1

        reset_requested = self._reset.re & self._reset.storage
        self.submodules.fsm = FSM(reset_state='fsm_reset')
        self.fsm.act(
            'fsm_reset',
            modules_reset.eq(1),
            NextState('idle')
        )
        self.fsm.act(
            'idle',
            self._idle.status.eq(True),
            # Peripheral is not running, so config allowed to change
            NextValue(self.cropper.column_start, column_start),
            NextValue(self.cropper.column_end, column_end),
            NextValue(self.cropper.row_start, row_start),
            NextValue(self.cropper.row_end, row_end),
            # When requested, start
            If(self._start_run.re & self._start_run.storage,
               NextState('waiting_for_data'),
               self.frame_gate.request.eq(1),
               ),
            # When requested, reset
            If(reset_requested, NextState('fsm_reset'))
        )
        self.fsm.act(
            'waiting_for_data',
            self._running.status.eq(True),
            self.packer.output.ready.eq(1),
            If(self.frame_gate.last, NextState('fsm_reset')),
            If(self.packer.output.valid,
               NextValue(self._pixels.status, self.packer.output.data),
               self._pixels_ready.status.eq(1),
               NextState('has_data')),
            # When requested, reset
            If(reset_requested, NextState('fsm_reset'))
        )
        self.fsm.act(
            'has_data',
            self._running.status.eq(True),
            self.packer.output.ready.eq(1),
            If(self.frame_gate.last, NextState('fsm_reset')),
            self._pixels_ready.status.eq(1),
            # When the CPU reads the pixel data (we write to the bus), clear the
            # pixels_ready bit and return to the waiting state.
            If(self._pixels.we,
               self._pixels_ready.status.eq(0),
               NextState('waiting_for_data')),
            # When requested, reset
            If(reset_requested, NextState('fsm_reset'))
        )
