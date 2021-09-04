# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Gateware for receiving data from the HM01B0 camera."""

from unittest import TestCase

from migen import (If, Module, Mux, Signal, Record, run_simulation,
                   ResetInserter, Cat, FSM, NextState, NextValue, passive)

from litex.soc.interconnect import stream, wishbone
from litex.soc.interconnect.csr import AutoCSR, CSRField, CSRStatus, CSRStorage
from litex.soc.cores import dma


# Defines the I/O input signals from the camera
_PAD_LAYOUT = [
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
        sigs = Record(_PAD_LAYOUT)
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
        cross = Record(_PAD_LAYOUT)
        self.comb += [
            cross.lvld.eq(pads.lvld),
            cross.fvld.eq(pads.fvld),
            cross.pclk.eq(pads.pclk),
            cross.data.eq(pads.data)]
        for _ in range(2):
            cross_next = Record(_PAD_LAYOUT)
            self.sync += cross_next.eq(cross)
            cross = cross_next
        return cross


class PixelProducerTest(TestCase):
    """Tests for the pixel producer class."""

    def setUp(self):
        self.pads = Record(_PAD_LAYOUT)
        self.dut = PixelProducer(pads=self.pads)

    def send_data(self, byte):
        yield self.pads.pclk.eq(0)
        for _ in range(4):
            yield
        yield self.pads.data.eq(byte)
        for _ in range(4):
            yield
        yield self.pads.pclk.eq(1)

    def read_pixel(self):
        yield self.dut.output.ready.eq(1)
        yield
        while not (yield self.dut.output.valid):
            yield
        self.assertTrue((yield self.dut.busy))
        result = (yield self.dut.output.data)
        yield self.dut.output.ready.eq(0)
        yield
        self.assertFalse((yield self.dut.output.valid))
        return result

    def test_it_sends_pixels(self):
        def run():
            # Send two pixels with FVLD up
            yield self.pads.fvld.eq(1)
            yield from self.send_data(111)
            while not (yield self.dut.output.valid):
                yield
            self.assertEqual(111, (yield from self.read_pixel()))
            yield from self.send_data(222)
            while not (yield self.dut.output.valid):
                yield
            self.assertEqual(222, (yield from self.read_pixel()))
            # Third pixel sent with fvld de-asserted should not be produced
            yield self.pads.fvld.eq(0)
            yield
            yield from self.send_data(333)
            for _ in range(10):
                self.assertFalse((yield self.dut.busy))
                self.assertFalse((yield self.dut.output.valid))
                yield
        run_simulation(self.dut, generators=[run()])


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


class CropperTest(TestCase):
    """Tests for the input cropper"""

    def setUp(self):
        self.dut = Cropper(raw_rows=10, raw_columns=10)

    def send_one_hundred_pixels(self):
        for _ in range(10):
            yield
        input = self.dut.input  # pylint: disable=redefined-builtin
        for i in range(100):
            yield input.data.eq(i)
            yield input.valid.eq(1)
            yield
            while not (yield input.ready):
                yield
            yield input.valid.eq(0)
            yield
        yield

    def check_expected(self, expected_output):
        output = self.dut.output
        yield output.ready.eq(1)
        for d in expected_output:
            while not (yield output.valid):
                yield
            self.assertEqual((yield output.data), d)
            yield

    def test_it_crops_8_rows_of_6(self):
        def run():
            yield self.dut.row_start.eq(1)
            yield self.dut.row_end.eq(8)
            yield self.dut.column_start.eq(2)
            yield self.dut.column_end.eq(7)
            yield
            yield from self.check_expected([
                12, 13, 14, 15, 16, 17,
                22, 23, 24, 25, 26, 27,
                32, 33, 34, 35, 36, 37,
                42, 43, 44, 45, 46, 47,
                52, 53, 54, 55, 56, 57,
                62, 63, 64, 65, 66, 67,
                72, 73, 74, 75, 76, 77,
                82, 83, 84, 85, 86, 87,
            ])
        run_simulation(self.dut,
                       generators=[self.send_one_hundred_pixels(), run()])


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


class WordPackerTest(TestCase):
    """Tests for the WordPacker."""

    def setUp(self):
        self.dut = WordPacker()

    def check_sequence(self, data):
        for (input, output) in data:  # pylint: disable=redefined-builtin
            i_valid, i_last, i_data, i_ready = input
            o_valid, o_last, o_data, o_ready = output
            yield self.dut.input.valid.eq(i_valid)
            yield self.dut.input.last.eq(i_last)
            yield self.dut.input.data.eq(i_data)
            yield self.dut.output.ready.eq(o_ready)
            yield
            if i_ready:
                self.assertEqual((yield self.dut.input.ready), i_ready)
            self.assertEqual((yield self.dut.output.valid), o_valid)
            if o_last:
                self.assertEqual((yield self.dut.output.last), o_last)
            if o_data:
                self.assertEqual((yield self.dut.output.data), o_data)

    def test_it_packs_words(self):
        X = None
        run_simulation(self.dut, generators=[
            self.check_sequence([
                # input: valid, last, data, ready
                # then output: valid, last, data, ready
                ((0, 0, 0x00, X), (0, X, X, 0)),
                ((1, 0, 0x10, 1), (0, X, X, 1)),
                ((1, 0, 0x20, 1), (0, X, X, 1)),
                ((1, 0, 0x30, 1), (0, X, X, 1)),
                ((1, 0, 0x40, 1), (1, 0, 0x10203040, 1)),
                ((1, 0, 0x50, 1), (0, X, X, 1)),
                ((1, 0, 0x60, 1), (0, X, X, 1)),
                ((1, 0, 0x70, 1), (0, X, X, 1)),
                ((1, 1, 0x80, 1), (1, 1, 0x50607080, 1)),
            ])])

    def test_it_can_handle_delays(self):
        X = None
        run_simulation(self.dut, generators=[
            self.check_sequence([
                # Send input quickly, but output not ready
                ((0, 0, 0x00, X), (0, X, X, 0)),
                ((1, 0, 0x10, 1), (0, X, X, 0)),
                ((1, 0, 0x20, 1), (0, X, X, 0)),
                ((1, 0, 0x30, 1), (0, X, X, 0)),
                ((1, 1, 0x40, 0), (1, 1, 0x10203040, 0)),
                ((1, 1, 0x40, 0), (1, 1, 0x10203040, 0)),
                ((1, 1, 0x40, 1), (1, 1, 0x10203040, 1)),
                # Same but also send input slowly
                ((0, 0, 0x00, 0), (0, X, X, 0)),
                ((1, 0, 0x10, 1), (0, X, X, 0)),
                ((0, 0, 0x00, 0), (0, X, X, 0)),
                ((0, 0, 0x00, 0), (0, X, X, 0)),
                ((1, 0, 0x20, 1), (0, X, X, 0)),
                ((0, 0, 0x00, 0), (0, X, X, 0)),
                ((0, 0, 0x00, 0), (0, X, X, 0)),
                ((1, 0, 0x30, 1), (0, X, X, 0)),
                ((0, 0, 0x00, 0), (0, X, X, 0)),
                ((1, 1, 0x40, 0), (1, 1, 0x10203040, 0)),
                ((1, 1, 0x40, 0), (1, 1, 0x10203040, 0)),
                ((1, 1, 0x40, 1), (1, 1, 0x10203040, 1)),
            ])])


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


class AddressGeneratorTest(TestCase):
    """Tests for the AddressGenerator."""

    def setUp(self):
        self.dut = AddressGenerator()

    def send_receive(self, value, expected_address, *,
                     last=False, expected_done=False):
        input = self.dut.input  # pylint: disable=redefined-builtin
        output = self.dut.output
        yield input.valid.eq(True)
        yield input.data.eq(value)
        yield input.last.eq(last)
        yield output.ready.eq(True)
        yield
        self.assertTrue((yield output.valid))
        self.assertTrue((yield input.ready))
        self.assertEqual((yield output.data), value)
        self.assertEqual((yield output.address), expected_address)
        self.assertEqual((yield self.dut.done), expected_done)
        yield input.valid.eq(False)
        yield output.ready.eq(False)
        yield

    def test_it_increments_addess(self):
        def run():
            yield self.dut.address.eq(0x4000)
            yield from self.send_receive(1111, 0x4000)
            yield from self.send_receive(2222, 0x4001)
            yield from self.send_receive(3333, 0x4002)
            yield from self.send_receive(4444, 0x4003)
            yield from self.send_receive(5555, 0x4004)
        run_simulation(self.dut, generators=[run()])

    def test_it_signals_done(self):
        def run():
            yield self.dut.address.eq(0x4000)
            yield from self.send_receive(0x1100_0000, 0x4000)
            yield from self.send_receive(0x2200_0000, 0x4001)
            yield from self.send_receive(0x3300_0000, 0x4002,
                                         last=True, expected_done=True)
        run_simulation(self.dut, generators=[run()])


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


class ItemCounterTest(TestCase):
    """Tests for the StreamCounter."""

    def setUp(self):
        self.dut = ItemCounter([('data', 8)])

    def test_it_counts(self):
        DATA = [
            ((0, 11, 0), 0),
            ((1, 22, 0), 0),
            ((1, 33, 1), 0),
            ((1, 44, 1), 1),
            ((1, 55, 1), 2),
            ((1, 66, 0), 3),
            ((1, 77, 0), 3),
            ((0, 88, 1), 3),
            ((1, 99, 1), 3),
            ((0, 11, 0), 4),
        ]

        def run():
            sink = self.dut.sink
            source = self.dut.source
            for (input, expected) in DATA:  # pylint: disable=redefined-builtin
                sink_valid, sink_data, source_ready = input
                yield sink.valid.eq(sink_valid)
                yield sink.data.eq(sink_data)
                yield source.ready.eq(source_ready)
                yield
                self.assertEqual((yield source.valid), sink_valid)
                self.assertEqual((yield source.data), sink_data)
                self.assertEqual((yield sink.ready), source_ready)
                self.assertEqual((yield self.dut.counter), expected)

        run_simulation(self.dut, generators=[run()])


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


class CameraControlIntegrationTest(TestCase):
    """Integration test for CameraControl"""
    # pylint: disable=protected-access

    RAW_COLUMNS = 20
    RAW_ROWS = 20

    def setUp(self):
        self.dma_words_written = 0
        self.pads = Record(_PAD_LAYOUT)
        self.bus = wishbone.Interface()
        self.dut = CameraControl(
            pads=self.pads,
            bus=self.bus,
            raw_columns=self.RAW_COLUMNS,
            raw_rows=self.RAW_ROWS)

    @passive
    def run_camera(self):
        """Simulates camera signal on pads"""
        def wait(n):
            for _ in range(n):
                yield
        pads = self.pads
        while True:
            yield from wait(100)
            yield pads.fvld.eq(1)
            for r in range(self.RAW_ROWS):
                yield pads.lvld.eq(1)
                yield from wait(10)
                for c in range(self.RAW_COLUMNS):
                    yield pads.pclk.eq(0)
                    # value is constructed from row and column number
                    yield pads.data.eq((r % 16) * 16 + (c % 16))
                    yield from wait(2)
                    yield pads.pclk.eq(1)
                    yield from wait(2)
                yield pads.pclk.eq(0)
                yield pads.lvld.eq(0)
                yield from wait(100)
            yield pads.fvld.eq(0)

    def expected_word_for(self, row, column):
        result = 0
        for i in range(4):
            b = (column + i) & 0xf
            b += (row & 0xf) << 4
            b += 128
            b &= 0xff
            result += b << (i * 8)
        return result

    @passive
    def check_data(self, addr_start, column_start,
                   column_count, row_start, row_count):
        """checks data received on bus"""
        bus = self.bus
        expected_address = addr_start
        column = column_start
        row = row_start
        while True:
            # Wait for a bus transaction
            waited = 0
            while not (yield bus.cyc & bus.stb):
                waited += 1
                if waited > 50000:
                    self.fail(f'no data after waiting {waited} cycles')
                yield
            # check transaction is a write with all necessary attributes
            self.assertTrue((yield self.bus.we))
            self.assertEqual((yield self.bus.adr), expected_address >> 2)
            self.assertEqual((yield self.bus.sel), 0xf)
            actual = (yield self.bus.dat_w)
            expected = self.expected_word_for(row, column)
            self.assertEqual(actual, expected,
                             f'{actual:08x} != {expected:08x}')
            yield self.bus.ack.eq(1)
            yield
            yield self.bus.ack.eq(0)
            yield
            self.dma_words_written += 1
            expected_address += 4
            column += 4
            if column == column_start + column_count:
                column = column_start
                row += 1
                if row == row_start + row_count:
                    row = row_start
                    expected_address = addr_start

    def set_control(self, addr_start, column_start,
                    column_count, row_start, row_count):
        yield from self.dut._column_start.write(column_start)
        yield from self.dut._column_count.write(column_count)
        yield from self.dut._row_start.write(row_start)
        yield from self.dut._row_count.write(row_count)
        yield from self.dut._base_address.write(addr_start)

    def test_it_receives_twice(self):
        addr_start = 0x10000
        column_start = 2
        column_count = 16
        row_start = 4
        row_count = 12
        pixels_per_frame = column_count * row_count
        words_per_frame = pixels_per_frame >> 2

        def run():
            # Check is idle
            self.assertFalse((yield from self.dut._running.read()))
            self.assertFalse((yield from self.dut._busy.read()))
            yield from self.set_control(addr_start, column_start,
                                        column_count, row_start, row_count)
            yield from self.dut._control.write(3)  # start and sign words
            yield
            # set control running, wait until done then check word count
            while (yield from self.dut._running.read()):
                yield
            self.assertEqual(self.dma_words_written, words_per_frame)
            self.assertEqual((yield from self.dut._pixels_cropped.read()),
                             pixels_per_frame)
            # wait for control to become un-busy and then start again
            while (yield from self.dut._busy.read()):
                yield
            yield
            # start again
            yield from self.dut._control.write(3)
            yield
            while (yield from self.dut._running.read()):
                yield
            self.assertEqual(self.dma_words_written, 2 * words_per_frame)

            # Wait a bit and check no additional words written
            for _ in range(300):
                yield
            self.assertEqual(self.dma_words_written, 2 * words_per_frame)

        run_simulation(
            self.dut,
            generators=[
                run(),
                self.run_camera(),
                self.check_data(addr_start, column_start,
                                column_count, row_start, row_count)])
