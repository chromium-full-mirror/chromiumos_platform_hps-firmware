# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

__doc__ = """A Stream abstraction
====================

Streams allows data to be transferred from one component to another.

A stream of data consists of a series of payloads transferred from an
upstream (aka producer or source) to a downstream (aka consumer or
sink). Each packet contains one payload. 

The upstream and downstream must operate in the same clock domain. They
communicate with these signals:

- payload: The data to be transferred. This may be a simple Signal or
  else a Record. Set by upstream.
- valid: Set by upstream to indicate that a valid payload is being
  presented.
- ready: Set by downstream to indicate that it is ready to accept a
  packet.

A transfer takes place when both of the valid and ready signals are
asserted on the same clock cycle. This handshake allows for a simple
flow control - upstreams are able to request that downstreams wait for
data, and downstreams are able to request for upstreams to pause data
production.

In order to ensure that there are no combinatorial cycles between
pipelines, the valid signal of a stream must not combinatorially
depend on the stream's ready signal. As a general principle, it is
also useful to avoid having ready combinatorially depend on the
stream's valid Signal.

There are cases where an upstream or downstream may not be able to
wait for a valid-ready handshake. For example, a video phy sink may
require valid data be presented at every clock cycle in order to
generate a signal for a monitor. If an upstream or downstream cannot
wait for a handshake, it declares this in its Definition.

This implementation was heavily influenced by the discussion at
https://github.com/amaranth-lang/amaranth/issues/317, and especially the
existing design of LiteX streams. It relies heavily on the amaranth Record class
for payload manipulation.

Major differences from LiteX Streams:

-  There are no "first" or "last" signals.
-  The payload is a single signal or Record named "payload".
"""

from .actor import BinaryCombinatorialActor
from .actor import BinaryPipelineActor
from .stream import connect
from .stream import Endpoint
from .stream import PayloadDefinition
