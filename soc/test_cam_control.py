# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Tests for code in cam_control.py"""

from unittest import TestCase

from migen import (Record, run_simulation, passive)

from litex.soc.interconnect import wishbone

from .cam_control import (PAD_LAYOUT, AddressGenerator, CameraControl,
                          Cropper, ItemCounter, PixelProducer, WordPacker)


class PixelProducerTest(TestCase):
    """Tests for the pixel producer class."""

    def setUp(self):
        self.pads = Record(PAD_LAYOUT)
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


class CameraControlIntegrationTest(TestCase):
    """Integration test for CameraControl"""
    # pylint: disable=protected-access

    RAW_COLUMNS = 20
    RAW_ROWS = 20

    def setUp(self):
        self.dma_words_written = 0
        self.pads = Record(PAD_LAYOUT)
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
