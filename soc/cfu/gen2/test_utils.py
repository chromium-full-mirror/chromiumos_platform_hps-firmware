# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Tests for uitls.py"""

import random

from amaranth import Module
from amaranth import Signal

from ..util import TestBase
from .utils import delay


class DelayTest(TestBase):
    """Tests the delay function."""

    def create_dut(self):
        module = Module()
        # pylint: disable=attribute-defined-outside-init
        self.in_ = Signal(8)
        # pylint: disable=attribute-defined-outside-init
        self.outs_ = delay(module, self.in_, 3)
        return module

    def test_it(self):
        # data with 3 zeros at end, since we are delaying by 3
        data = [random.randrange(256) for _ in range(20)] + [0] * 3

        def process():
            # pylint: disable=consider-using-enumerate
            for i in range(len(data)):
                yield self.in_.eq(data[i])
                yield
                for j in range(3):
                    self.assertEqual((yield self.outs_[j]), data[i - j])

        self.run_sim(process, False)
