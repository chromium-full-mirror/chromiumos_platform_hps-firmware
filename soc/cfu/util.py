# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Utility functions for Amaranth logic."""

import abc
import unittest

from amaranth import Elaboratable
from amaranth import Module
from amaranth import Signal
from amaranth.build import Platform
from amaranth.sim import Simulator


def tree_sum(l):
    """Sums a list of values in a tree rather than sequentially.

    This is helpful to some synthesis tools. Others automatically recognize
    a series of additions and build special, optimized logic.
    """
    l = list(l)
    if len(l) == 1:
        return l[0]
    half = len(l) // 2
    return tree_sum(l[:half]) + tree_sum(l[half:])


# pylint: disable=docstring-misnamed-args
def pack_vals(*values, offset=0, bits=8):
    """Packs single values into a word in little endian order.

    Args:
        values: individual values to be packed
        offset: added to each value before packing
        bits: number of bits in each value
    """
    mask = (1 << bits) - 1
    result = 0
    for i, v in enumerate(values):
        result += ((v + offset) & mask) << (i * bits)
    return result


class SimpleElaboratable(Elaboratable, abc.ABC):
    """Simplified Elaboratable interface

    Widely, but not generally applicable. Suitable for use with
    straight-forward blocks of logic in a single domain.

    Attributes:
        m (Module):
            The resulting module
        platform:
            The platform for this elaboration
    """

    @abc.abstractmethod
    def elab(self, m: Module):
        """Alternate elaborate interface"""
        return NotImplementedError()

    def elaborate(self, platform: Platform):
        # pylint: disable=attribute-defined-outside-init
        self.m = Module()
        # pylint: disable=attribute-defined-outside-init
        self.platform = platform
        self.elab(self.m)
        return self.m


class _PlaceholderSyncModule(SimpleElaboratable):
    """A module that does something arbirarty with synchronous logic

    This is used by TestBase to stop Amaranth from complaining if our DUT
    doesn't contain any synchronous logic.
    """

    def elab(self, m):
        state = Signal(1)
        m.d.sync += state.eq(~state)


class TestBase(unittest.TestCase):
    """Base class for testing an Amaranth module.

    The module can use sync, comb or both.
    """

    def setUp(self):
        # Create DUT and add to simulator
        self.m = Module()
        self.dut = self.create_dut()
        self.m.submodules["dut"] = self.dut
        self.m.submodules["placeholder"] = _PlaceholderSyncModule()
        self.sim = Simulator(self.m)

    def create_dut(self):
        """Returns an instance of the device under test"""
        raise NotImplementedError

    def add_process(self, process):
        """Add main test process to the simulator"""
        self.sim.add_sync_process(process)

    def add_sim_clocks(self):
        """Add clocks as required by sim."""
        self.sim.add_clock(1, domain="sync")

    def run_sim(self, process, write_trace=False):
        self.add_process(process)
        self.add_sim_clocks()
        if write_trace:
            with self.sim.write_vcd("zz.vcd", "zz.gtkw"):
                self.sim.run()
            # Discourage commiting code with tracing active
            self.fail(
                "Simulation tracing active. "
                "Turn off after debugging complete."
            )
        else:
            self.sim.run()


class RegisteredReadPort(SimpleElaboratable):
    """Wraps a ReadPort and delays data output by one cycle.

    This then behaves like RAM with registered outputs.

    Attributes:
        addr (Signal(range(max_depth)), out):
            The address to send to the memory read port
        data (Signal(32), out):
            The data from memory
        en (Signal or Const, in):
            Whether the read port is enabled
    """

    def __init__(self, inner):
        self.inner = inner
        self.addr = Signal(self.inner.addr.shape())
        self.en = Signal()
        self.data = Signal(self.inner.data.shape())

    def elab(self, m):
        m.submodules += self.inner
        m.d.comb += self.inner.en.eq(self.en)
        m.d.comb += self.inner.addr.eq(self.addr)
        m.d.sync += self.data.eq(self.inner.data)
