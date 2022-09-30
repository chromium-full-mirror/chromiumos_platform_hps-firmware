# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Tests for filter.py"""

from ..util import TestBase
from .filter import FilterStore


class FilterStoreTest(TestBase):
    """Tests FilterStore class"""

    def create_dut(self):
        return FilterStore()

    def read_three_times(self):
        dut = self.dut
        yield dut.start.eq(1)
        yield
        yield dut.start.eq(0)
        yield
        yield
        yield
        for _ in range(3):
            for addr in range(512):
                for store in range(2):
                    expected = 10000 + 1000 * store + (addr - store) % 512
                    self.assertEqual(
                        (yield dut.values_out[store]),
                        expected,
                        f"store={store}, addr={addr}",
                    )
                yield

    def test_it(self):
        dut = self.dut

        def process():
            # Write 512 valuees to memory 0, then 1
            yield dut.size.eq(512)
            write = dut.write_input
            for store in range(2):
                yield write.payload.store.eq(store)
                for addr in range(512):
                    yield write.valid.eq(1)
                    yield write.payload.addr.eq(addr)
                    yield write.payload.data.eq(10000 + 1000 * store + addr)
                    yield
                    self.assertTrue((yield write.ready))
                    yield write.valid.eq(0)
            # Read all values three times
            yield
            yield from self.read_three_times()

            # Allow to continue a while
            for _ in range(123):
                yield

            # Fetch values three more times
            yield from self.read_three_times()

        self.run_sim(process, False)
