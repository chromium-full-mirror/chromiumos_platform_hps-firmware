# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Unit tests for cam_control.py"""

import random
from unittest import TestCase

from migen.sim.core import passive
from migen import Record, run_simulation

from . import cam_control


class PixelProducerTest(TestCase):
    """Tests for the pixel producer class."""

    _FRAME_PIXELS = 5

    def setUp(self):
        self.pads = Record(cam_control.PAD_LAYOUT)
        self.dut = cam_control.PixelProducer(
            pads=self.pads, num_pixels=self._FRAME_PIXELS)

    def send_pixel(self, byte):
        yield
        yield
        yield self.pads.pclk.eq(0)
        yield self.pads.data.eq(byte)
        for _ in range(4):
            yield
        yield self.pads.pclk.eq(1)
        yield
        yield

    def assert_stream_content(self, expected):
        yield
        for (value, first, last) in expected:
            yield self.dut.output.ready.eq(1)
            while not (yield self.dut.output.valid):
                yield
            self.assertEqual((yield self.dut.output.data), value)
            self.assertEqual((yield self.dut.output.first), first)
            self.assertEqual((yield self.dut.output.last), last)
            yield self.dut.output.ready.eq(0)
            yield
        self.assertFalse((yield self.dut.output.valid))

    def test_it_sends_a_frame(self):
        frame = [random.randrange(256) for _ in range(self._FRAME_PIXELS)]
        expected = [
            (frame[0], True, False),
            *[(p, False, False) for p in frame[1:-1]],
            (frame[-1], False, True)
        ]

        def run():
            yield
            # Set FVLD up, then send ten pixels
            yield self.pads.fvld.eq(1)
            for pixel in frame:
                yield from self.send_pixel(pixel)
            for _ in range(4):
                yield
            yield self.pads.fvld.eq(0)
        run_simulation(
            self.dut,
            generators=[run(), self.assert_stream_content(expected)])

    def test_it_handles_fvld(self):
        """Checks that pixels sent while fvld lowered are not recorded."""
        PAD_DATA = [
            # (fvld, data to send)
            (False, 0),
            (False, 1),
            (True, 2),
            (True, 3),
            (False, 4),
            (True, 5),
            (False, 6),
            (False, 7),
            (False, 8),
        ]
        EXPECTED = [
            (2, True, False),
            (3, False, False),
            (5, True, False),
        ]

        def run():
            for (fvld, pixel) in PAD_DATA:
                yield self.pads.fvld.eq(fvld)
                yield from self.send_pixel(pixel)
        run_simulation(
            self.dut,
            generators=[run(), self.assert_stream_content(EXPECTED)])

    def test_it_sets_first_and_last(self):
        """Checks that first and last are handled correctly"""
        PAD_DATA = [
            # (fvld, data to send)
            (False, 0),
            (False, 1),
            (True, 2),
            (True, 3),
            (True, 4),
            (True, 5),
            (True, 6),
            (True, 7),
            (False, 8),
            (False, 9),
            (False, 10),
            (True, 11),
            (True, 12),
            (True, 13),
            (True, 14),
            (True, 15),
            (False, 16),
            (False, 17),
            (True, 18),
            (True, 19),
            (False, 20),
            (False, 21),
        ]
        EXPECTED = [
            # First frame is 6 pixels long instead of expected 5
            (2, True, False),
            (3, False, False),
            (4, False, False),
            (5, False, False),
            # This pixel is last by count
            (6, False, True),
            (7, False, False),
            # Next frame is expected 5 pixels
            (11, True, False),
            (12, False, False),
            (13, False, False),
            (14, False, False),
            (15, False, True),
            # Final frame is just two pixels
            (18, True, False),
            (19, False, False),
        ]

        def run():
            for (fvld, pixel) in PAD_DATA:
                yield self.pads.fvld.eq(fvld)
                yield from self.send_pixel(pixel)
        run_simulation(
            self.dut,
            generators=[
                run(),
                self.assert_stream_content(EXPECTED)])


def send(stream, data, first=False, last=False):
    """Sends data on a stream"""
    yield stream.data.eq(data)
    yield stream.first.eq(first)
    yield stream.last.eq(last)
    yield stream.valid.eq(1)
    yield
    while not (yield stream.ready):
        yield
    yield stream.valid.eq(0)


def receive(stream):
    """Receives data from a stream"""
    yield stream.ready.eq(1)
    yield
    while not (yield stream.valid):
        yield
    result = ((yield stream.data), (yield stream.first), (yield stream.last))
    yield stream.ready.eq(0)
    return result


def pulse(signal):
    """Sends a single cycle pulse on signal"""
    yield signal.eq(1)
    yield
    yield signal.eq(0)


class WordPackerTest(TestCase):
    """Tests for the WordPacker."""

    def setUp(self):
        self.dut = cam_control.WordPacker()

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
                ((0, 0, 0x01, X), (0, X, X, 0)),
                ((0, 0, 0x02, X), (0, X, X, 0)),
                ((0, 0, 0x00, X), (0, X, X, 0)),
                ((1, 0, 0x10, 1), (0, X, X, 1)),
                ((1, 0, 0x20, 1), (0, X, X, 1)),
                ((1, 0, 0x30, 1), (0, X, X, 1)),
                ((1, 0, 0x40, 1), (1, 0, 0x40302010, 1)),
                ((1, 0, 0x50, 1), (0, X, X, 1)),
                ((1, 0, 0x60, 1), (0, X, X, 1)),
                ((1, 0, 0x70, 1), (0, X, X, 1)),
                ((1, 1, 0x80, 1), (1, 1, 0x80706050, 1)),
            ])])


class CameraControlIntegrationTest(TestCase):
    """Integration test for CameraControl"""
    # pylint: disable=protected-access

    RAW_COLUMNS = 12
    RAW_ROWS = 12

    def setUp(self):
        self.pads = Record(cam_control.PAD_LAYOUT)
        self.dut = cam_control.CameraControl(
            pads=self.pads,
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
                yield from wait(10)
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

    def check_data(self, start_row, num_columns):
        """checks data received from camera"""
        column = 0
        row = 0
        while row < self.RAW_ROWS:
            # Wait for a bus transaction
            waited = 0
            while not (yield from self.dut._pixels_ready.read()):
                waited += 1
                if waited > 1000:
                    self.fail(f'no data after waiting {waited} cycles')
                yield
            if row < start_row:
                yield from self.dut._wait_row.write(1)
                yield
                row += 1
                continue
            # check transaction is a write with all necessary attributes
            actual = (yield from self.dut._pixels.read())
            expected = self.expected_word_for(row, column)
            self.assertEqual(
                actual, expected,
                f'{actual:08x} != {expected:08x} @{row}, {column}')
            # Reading the data should have cleared _pixels_ready.
            yield
            self.assertFalse((yield from self.dut._pixels_ready.read()))
            column += 4
            if column == num_columns:
                yield from self.dut._wait_row.write(1)
                yield
                column = 0
                row += 1
        # Reset
        yield from self.dut._reset.write(1)
        yield
        # Wait until the frame finishes.
        while (yield from self.dut._running.read()):
            pass

    def test_it_receives_twice(self):
        def run():
            yield
            yield
            yield from self.dut._start_run.write(1)  # start
            yield
            # set control running, wait until done then check word count
            yield from self.check_data(4, 8)
            yield from self.dut._start_run.write(0)  # start
            yield

            # start again
            yield from self.dut._start_run.write(1)  # start
            yield
            yield from self.check_data(4, 8)

            # Wait a bit and check no additional words become available.
            for _ in range(300):
                yield
                self.assertFalse((yield self.dut._pixels_ready.status))

        run_simulation(
            self.dut,
            generators=[
                run(),
                self.run_camera()]
        )
