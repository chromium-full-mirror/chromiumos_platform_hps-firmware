# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Gateware for receiving data from the HM01B0 camera."""

from migen import (If, Module, Mux, Signal, Record, ResetInserter, Cat, FSM,
                   NextState, NextValue)

from litex.soc.interconnect import stream
from litex.soc.interconnect.csr import AutoCSR, CSRField, CSRStatus, CSRStorage
from litex.soc.cores import dma


# Defines the I/O input signals from the camera
PAD_LAYOUT = [
    ('lvld', 1),
    ('fvld', 1),
    ('pclk', 1),
    ('data', 8),
]

# Layouts for various streams
_PIXEL_STREAM_LAYOUT = stream.EndpointDescription([('data', 8)])
_WORD_STREAM_LAYOUT = stream.EndpointDescription([('data', 32)])
_DMA_LAYOUT = [
    ('address', 30),
    ('data', 32),
]

# Raw camera image dimensions
# Must match camera configuration - eg 324x244 (QVGA) or 324x324
_RAW_IMAGE_COLUMNS = 324
_RAW_IMAGE_ROWS = 244


class PixelProducer(Module):
    """Receives data from camera I/O lines and puts it on a stream

    Produces one data item per pixel received

    Does not respect backpressure from the output stream.
    """

    def __init__(self, *, pads):
        self.output = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.busy = Signal()
        sigs = Record(PAD_LAYOUT)
        self.comb += sigs.eq(self._cross_domain(pads))
        self.comb += self.busy.eq(sigs.fvld)

        last_pclk = Signal()
        self.sync += last_pclk.eq(sigs.pclk)
        pclk_rising = sigs.pclk & ~last_pclk

        # Set Data out when pclk rising
        self.sync += If(sigs.fvld & pclk_rising,
                        self.output.data.eq(sigs.data),
                        self.output.valid.eq(1))

        # Lower valid after transfer
        self.sync += If(self.output.ready & self.output.valid,
                        self.output.valid.eq(0))

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
    """Optionally adds 128 to incoming data.

    This has the effect of converting the unsigned 0-255 representation
    to a signed -128-127 representation.
    """

    def __init__(self):
        # sink is input, source is output
        self.sink = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        self.source = stream.Endpoint(_PIXEL_STREAM_LAYOUT)
        super().__init__(latency=1)
        self.enable_add = Signal()

        self.sync += self.source.data.eq(self.sink.data +
                                         Mux(self.enable_add, 128, 0))


class WordPacker(Module):
    """Packs pixel values into a word.

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
                Cat(self.input.data, byte2, byte1, byte0)),
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


class AddressGenerator(Module):
    """Generates incrementing addresses.

    Adds addresses to a piece of data, allowing the data to be written
    to memory via DMA.
    """

    def __init__(self):
        self.address = Signal(30)  # Word address, not byte address
        self.done = Signal()
        self.input = stream.Endpoint(_WORD_STREAM_LAYOUT)
        self.output = stream.Endpoint(_DMA_LAYOUT)

        # Pass input stream data to output and add address
        self.comb += [
            self.output.valid.eq(self.input.valid),
            self.output.data.eq(self.input.data),
            self.output.address.eq(self.address),
            self.input.ready.eq(self.output.ready),
        ]

        transferring = self.output.valid & self.output.ready

        # Increment address on transfer out
        self.sync += If(transferring, self.address.eq(self.address + 1))

        # Signal done when last input transferred out
        self.comb += self.done.eq(self.input.last & transferring)


class ItemCounter(stream.CombinatorialActor):
    """Counts items as they pass through the stream."""

    def __init__(self, layout):
        self.sink = stream.Endpoint(layout)
        self.source = stream.Endpoint(layout)
        super().__init__()
        self.counter = Signal(32)
        self.comb += self.source.payload.eq(self.sink.payload)
        self.sync += If(self.sink.ready & self.sink.valid,
                        self.counter.eq(self.counter + 1))


class CameraControl(Module, AutoCSR):
    """Handles outputs from the camera and outputs bytes of data to memory."""

    def __init__(self, *, pads, bus, raw_columns=_RAW_IMAGE_COLUMNS,
                 raw_rows=_RAW_IMAGE_ROWS):
        self._running = CSRStatus(1, description='Set while DMA is running.')
        self._busy = CSRStatus(
            1,
            description='Set while camera is busy sending data.')
        self._control = CSRStorage(fields=[
            CSRField(
                'start',
                size=1,
                offset=0,
                description='Writing a 1 to this field while not ' +
                'running and not busy starts a DMA.'),
            CSRField(
                'dma_signed',
                size=1,
                offset=1,
                description='Set to adjust pixel values to ' +
                'signed representation.'),
            CSRField(
                'stop',
                size=1,
                offset=2,
                description='Writing a 1 bit to this field will ' +
                'force the peripheral to stop running.'),
        ], name='control')
        self._column_start = CSRStorage(
            9, description='Index of column on which to start')
        self._column_count = CSRStorage(
            9, description='Number of columns to read')
        self._row_start = CSRStorage(
            9, description='Index of input row on which to start')
        self._row_count = CSRStorage(
            9, description='Number of consecutive rows to read')
        self._base_address = CSRStorage(
            32, description='Base address to which to write data')
        self._pixels_read = CSRStatus(
            32, description='Number of pixels read from camera')
        self._pixels_cropped = CSRStatus(
            32, description='Number of pixels cropped from image')

        # Contruct all modules
        self.submodules.producer = PixelProducer(pads=pads)
        self.submodules.read_counter = ResetInserter()(
            ItemCounter(_PIXEL_STREAM_LAYOUT))
        self.submodules.cropper = ResetInserter()(
            Cropper(raw_columns, raw_rows))
        self.submodules.crop_counter = ResetInserter()(
            ItemCounter(_PIXEL_STREAM_LAYOUT))
        self.submodules.adder = Add128Pipeline()
        self.submodules.fifo = stream.SyncFIFO(_PIXEL_STREAM_LAYOUT, depth=4)
        self.submodules.packer = WordPacker()
        self.submodules.addresser = AddressGenerator()
        self.submodules.writer = dma.WishboneDMAWriter(bus=bus)

        # Connect streams
        self.comb += [
            self.producer.output.connect(self.read_counter.sink),
            self.read_counter.source.connect(self.cropper.input),
            self.cropper.output.connect(self.crop_counter.sink),
            self.crop_counter.source.connect(self.adder.sink),
            self.adder.source.connect(self.fifo.sink),
            self.fifo.source.connect(self.packer.input),
            self.packer.output.connect(self.addresser.input),
            self.addresser.output.connect(self.writer.sink),
        ]

        # Track sensor busy status
        busy = self._busy.status
        self.comb += busy.eq(self.producer.busy)

        # Transition out of running state as soon as the addresser is
        # done. We do this here instead of at frame end in order to
        # allow processing of the received data to begin.
        running = self._running.status
        self.sync += If(running & self.addresser.done, running.eq(0))

        # Transition into running when (a) not already running, (b) the
        # sensor is not mid-frame and (c) a start is requested. A start
        # requested while already running or the sensor is busy will
        # fail.
        # On start, transfer values from control registers to the cores
        # that use those values.
        self.sync += If(
            ~running & ~busy & self._control.re & self._control.fields.start,
            running.eq(1),
            self.cropper.column_start.eq(
                self._column_start.storage),
            self.cropper.column_end.eq(
                self._column_start.storage + self._column_count.storage - 1),
            self.cropper.row_start.eq(
                self._row_start.storage),
            self.cropper.row_end.eq(
                self._row_start.storage + self._row_count.storage - 1),
            self.adder.enable_add.eq(
                self._control.fields.dma_signed),
            self.addresser.address.eq(self._base_address.storage[2:]))

        # Forced stop in any circumstance
        self.sync += If(self._control.re & self._control.fields.stop,
                        running.eq(0))

        # While not running, holds the cropper in reset so no new values
        # are sent downstream to be DMAed.
        # Also resets pipeline elements that have counters.
        self.comb += self.cropper.reset.eq(~running)
        self.comb += self.read_counter.reset.eq(~running)
        self.comb += self.crop_counter.reset.eq(~running)

        # Transfer debug counter values to registers
        self.sync += If(running,
                        self._pixels_read.status.eq(self.read_counter.counter),
                        self._pixels_cropped.status.eq(
                            self.crop_counter.counter))
