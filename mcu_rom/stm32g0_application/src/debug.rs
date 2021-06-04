// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use mcu_common::DEBUG_BYTES_PER_COMMAND;
use mcu_common::DEBUG_COMMAND_START;
use rtt_target::DownChannel;

pub struct DebugCommandHandler {
    channel: DownChannel,

    buffer: [u8; DEBUG_BYTES_PER_COMMAND],
    bytes_received: usize,
}

impl DebugCommandHandler {
    pub(crate) fn new(channel: DownChannel) -> Self {
        Self {
            channel,
            buffer: [0u8; DEBUG_BYTES_PER_COMMAND],
            bytes_received: 0,
        }
    }

    /// Writes the next debug command into `buffer`. Leaves `buffer` unchanged
    /// if there is no next command, or if it is incomplete.
    pub(crate) fn get_debug_command(&mut self, buffer: &mut [u8]) {
        let r = self.channel.read(&mut self.buffer[self.bytes_received..]);
        self.bytes_received += r;
        while self.bytes_received >= 1 && self.buffer[0] != DEBUG_COMMAND_START {
            self.buffer.copy_within(1..DEBUG_BYTES_PER_COMMAND, 0);
            self.bytes_received -= 1;
            self.bytes_received += self.channel.read(&mut self.buffer[self.bytes_received..]);
        }
        if self.bytes_received == DEBUG_BYTES_PER_COMMAND {
            buffer[0..DEBUG_BYTES_PER_COMMAND - 1].clone_from_slice(&self.buffer[1..]);
            self.bytes_received = 0;
        }
    }
}
