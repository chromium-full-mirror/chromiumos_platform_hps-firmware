// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use mcu_common::McuDebugCommand;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use mcu_common::DEBUG_COMMAND_START;
use rtt_target::rprintln;
use rtt_target::DownChannel;

pub struct DebugCommandHandler {
    channel: DownChannel,

    buffer: [u8; DEBUG_BYTES_PER_COMMAND],
    bytes_received: usize,
}

pub(crate) struct McuDebugCommandInfo {
    pub(crate) kind: McuDebugCommand,
    pub(crate) arg: u16,
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
            buffer.clone_from_slice(&self.buffer[1..4]);
            self.bytes_received = 0;
        }
    }

    pub(crate) fn get_mcu_debug_command(&mut self) -> Option<McuDebugCommandInfo> {
        let mut cmd_buf = [0u8; 3];
        self.get_debug_command(&mut cmd_buf);
        if cmd_buf[0] == 0 {
            return None;
        }
        let kind = match McuDebugCommand::try_from(cmd_buf[0]) {
            Ok(cmd) => cmd,
            Err(_) => {
                rprintln!("Unknown MCU command {}", cmd_buf[0]);
                return None;
            }
        };
        let arg = u16::from_le_bytes([cmd_buf[1], cmd_buf[2]]);
        Some(McuDebugCommandInfo { kind, arg })
    }
}
