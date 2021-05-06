# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Patches one pin through to another."""

from migen import Module

class Patch(Module):
    """Patches one pin through to another."""

    def __init__(self, input_pin, output_pin):
        self.comb += output_pin.eq(input_pin)
