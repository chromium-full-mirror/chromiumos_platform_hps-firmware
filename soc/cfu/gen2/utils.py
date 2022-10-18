# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Utility functions."""

from amaranth import Signal
from amaranth import unsigned


def unsigned_upto(maximum_value):
    """Creates a shape of a size to hold maximum_value"""
    return unsigned(maximum_value.bit_length())


def delay(m, input_, cycles):
    result = [input_]
    for i in range(cycles):
        delayed = Signal.like(input_, name=f"{input_.name}_d{i+1}")
        m.d.sync += delayed.eq(result[-1])
        result.append(delayed)
    return result


def as_signed_int32_array(byte_array):
    """Interprets array of byte values as signed 32 bit ints."""

    def int32(a, b, c, d):
        u = a + (b << 8) + (c << 16) + (d << 24)
        return u if u < (2**31) else (u - 2**32)

    return [int32(*byte_array[i : i + 4]) for i in range(0, len(byte_array), 4)]


def as_unsigned_int32_array(byte_array):
    """Interprets array of byte values as unsigned 32 bit ints."""

    def uint32(a, b, c, d):
        return a + (b << 8) + (c << 16) + (d << 24)

    return [
        uint32(*byte_array[i : i + 4]) for i in range(0, len(byte_array), 4)
    ]
