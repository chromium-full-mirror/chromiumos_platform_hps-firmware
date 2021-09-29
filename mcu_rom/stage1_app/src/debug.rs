// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use i2c_protocol::Buffer;
use i2c_protocol::WriteMemoryEvent;
use mcu_common::memory_banks;
use mcu_common::McuDebugCommand;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use mcu_common::DEBUG_COMMAND_START;
use rtt_target::rprintln;
use rtt_target::DownChannel;

pub struct DebugCommandHandler {
    channel: DownChannel,

    buffer: [u8; DEBUG_BYTES_PER_COMMAND],
    bytes_received: usize,
    pending: Option<McuDebugCommandInfo>,
}

pub(crate) struct McuDebugCommandInfo {
    pub(crate) kind: McuDebugCommand,
    pub(crate) arg: u16,
    pub(crate) write_request: Option<WriteMemoryEvent>,
}

impl DebugCommandHandler {
    pub(crate) fn new(channel: DownChannel) -> Self {
        Self {
            channel,
            buffer: [0u8; DEBUG_BYTES_PER_COMMAND],
            bytes_received: 0,
            pending: None,
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
            buffer[..3].clone_from_slice(&self.buffer[1..4]);
            self.bytes_received = 0;
        }
    }

    pub(crate) fn get_mcu_debug_command(&mut self) -> Option<McuDebugCommandInfo> {
        // If we're in the process of receiving a write request, then receive
        // bytes into the appropriate buffer.
        if let Some(command) = self.pending.as_mut() {
            // The following upwrap must succeed because we don't put anything
            // into `pending` without it having a write request.
            let write_request = command.write_request.as_mut().unwrap();
            let r = self
                .channel
                .read(&mut write_request.data[self.bytes_received..]);
            self.bytes_received += r;
            if self.bytes_received == write_request.data.len() {
                self.bytes_received = 0;
                return self.pending.take();
            }
            return None;
        }
        // Receive a regular command.
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
        let mut command = McuDebugCommandInfo {
            kind,
            arg,
            write_request: None,
        };
        if command.kind == McuDebugCommand::WriteSpiFlash {
            if let Some(mut buffer) = Buffer::new() {
                buffer.resize(Buffer::CAPACITY);
                command.write_request = Some(WriteMemoryEvent {
                    // Note, bank is not actually used by the caller, however if
                    // it were, FPGA_BITSTREAM represents the start of the SPI
                    // flash, so would do what we wanted.
                    bank: memory_banks::FPGA_BITSTREAM,
                    address: arg as u32 * 256,
                    data: buffer,
                });
            } else {
                rprintln!("Attempt to write SPI flash when buffer is unavailable");
                return None;
            }
            self.pending = Some(command);
            None
        } else {
            Some(command)
        }
    }
}
