# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""FIFO with stream interfaces."""

from amaranth import Module
from amaranth import Signal
from amaranth.lib.fifo import SyncFIFOBuffered

from ..util import SimpleElaboratable
from .stream import Endpoint


class StreamFifo(SimpleElaboratable):
    """A FIFO with stream interfaces.

    This module can be parametrized as follows:

    type: PayloadDefinition
      The type of the item that will be passed through the FIFO
    depth:
      The number of items contained by the fifo

    Attributes:
        input (Endpoint(type), in):
            Incoming stream of items
        output (Endpoint(type), out):
            Outgoing stream of items
        r_level (Signal(depth.bit_length())):
            Number of items available for reading
    """

    # pylint: disable=redefined-builtin
    def __init__(self, *, type, depth):
        self.depth = depth
        self.input = Endpoint(type)
        self.output = Endpoint(type)
        self.r_level = Signal(depth.bit_length())

    def elab(self, m: Module):
        m.submodules.wrapped = fifo = SyncFIFOBuffered(
            depth=self.depth, width=len(self.input.payload)
        )

        m.d.comb += [
            fifo.w_en.eq(self.input.valid),
            fifo.w_data.eq(self.input.payload),
            self.input.ready.eq(fifo.w_rdy),
            self.output.valid.eq(fifo.r_rdy),
            self.output.payload.eq(fifo.r_data),
            fifo.r_en.eq(self.output.ready),
            self.r_level.eq(fifo.r_level),
        ]
