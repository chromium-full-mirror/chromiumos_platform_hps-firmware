# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Base stream interfaces."""

from amaranth import Shape
from amaranth.hdl.rec import Layout
from amaranth.hdl.rec import Record


__all__ = ["PayloadDefinition", "Endpoint", "connect"]


class PayloadDefinition:
    """Defines a stream's payload

    Attributes:

        payload_type (Shape or Layout):
            The type of the payload.

        stream_layout (Layout):
            A record Layout for a stream containing the given payload.
    """

    @staticmethod
    def cast(obj, *, src_loc_at=0):
        """Returns a definition.

        Args:
            obj:
                PayloadDefinition or something that can be used in Layout -
                a Shape, a Layout or an iterable of tuples.
            src_loc_at:
                Number of stack frames to look upwards for source location.
        """
        if isinstance(obj, PayloadDefinition):
            return obj
        return PayloadDefinition(payload_type=obj, src_loc_at=src_loc_at + 1)

    def __init__(self, *, payload_type, src_loc_at=0):
        """Constructor.

        Args:
            payload_type (Shape or Layout):
                The type of the payload.
            src_loc_at:
                Number of stack frames to look upwards for source location.
        """
        self.payload_type = payload_type
        self.stream_layout = Layout(
            [
                ("valid", Shape()),
                ("ready", Shape()),
                ("payload", payload_type),
            ],
            src_loc_at=1 + src_loc_at,
        )


class Endpoint:
    """One endpoint of a stream."""

    def __init__(self, definition=None, *, name=None, src_loc_at=0):
        """Inits Endpoint.

        Args:
            definition (StreamDefintion):
                Specifies the payload type and other parameters of this type.
            name:
                Name to use for derived signals.
            src_loc_at:
                Number of stack frames to look upwards for source location.
        """
        self.definition = PayloadDefinition.cast(definition)
        self._record = Record(
            self.definition.stream_layout, name=name, src_loc_at=1 + src_loc_at
        )
        self.valid = self._record.valid
        self.ready = self._record.ready
        self.payload = self._record.payload

    @staticmethod
    def like(other):
        return Endpoint(other.definition)

    def is_transferring(self):
        """Is a transfer taking place this cycle?

        True iff valid and ready are both asserted.
        """
        return self.valid & self.ready


def connect(from_endpoint, to_endpoint):
    """Connects an upstream endpoint to a downstream endpoint.

    This is a convenience function only. Endpoint users may instead
    choose to write the 3 eq statements required to make a connection.

    Example uses:

    m.d.comb += connect(one_components_output, another_components_input)
    m.d.comb += connect(my_input, child_input)

    Args:
        from_endpoint:
            The upstream side of the stream. Presents payload and valid.
        to_endpoint:
            The downstream side of the stream. Presents ready.

    Returns:
        A list of assignment statements to be used combinatorially.
    """
    return [
        to_endpoint.valid.eq(from_endpoint.valid),
        to_endpoint.payload.eq(from_endpoint.payload),
        from_endpoint.ready.eq(to_endpoint.ready),
    ]
