// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod commands;
pub mod status;

use crate::application_state::Configuration;
use crate::application_state::Status;
use crate::ApplicationState;
use core::convert::TryFrom;
use mcu_common::MemBlock;

pub use self::commands::Action;

/// Size of buffer used for SPI communcation between the FPGA and the MCU.
pub const BUFFER_SIZE: usize = 256;

pub struct Request {
    /// An action that should be performed before we send the response. Will be
    /// absent if the request was able to be processed entirely by accessing
    /// state.
    pub action: Action,
    response_code: u8,
}

impl Request {
    pub fn from_received_packet(buffer: &MemBlock) -> Self {
        let command_code = buffer[0];
        let mut response_code = status::OK;
        let action = match Action::try_from(command_code) {
            Ok(Action::Poll) => {
                response_code = status::READY;
                Action::Poll
            }
            Ok(command) => command,
            Err(_) => {
                response_code = status::ERROR;
                Action::Poll
            }
        };
        Request {
            action,
            response_code,
        }
    }

    pub fn prepare_to_send(&self, buffer: &mut MemBlock) {
        // Copy command code from its location in the request to its location in
        // the response.
        buffer[1] = buffer[0];
        buffer[0] = self.response_code;
    }
}

pub fn read_config_registers(buffer: &mut MemBlock, state: &ApplicationState) {
    let payload_out = &mut buffer[2..];
    // safety: We're careful not to write beyond the end of the buffer.
    unsafe {
        core::ptr::copy_nonoverlapping::<u8>(
            &state.configuration as *const _ as *const u8,
            payload_out.as_mut_ptr(),
            usize::min(payload_out.len(), core::mem::size_of::<Configuration>()),
        );
    }
}

pub fn write_status_registers(buffer: &MemBlock, state: &mut ApplicationState) {
    let payload_in = &buffer[1..];
    // safety: We're careful not to read beyond the end of the buffer. Status
    // contains only types for which every bit pattern is valid. In particular,
    // it contains no bools.
    unsafe {
        core::ptr::copy_nonoverlapping::<u8>(
            payload_in.as_ptr(),
            &mut state.status as *mut _ as *mut u8,
            usize::min(payload_in.len(), core::mem::size_of::<Status>()),
        );
    }
}

pub fn report_boot(state: &mut ApplicationState, buffer: &MemBlock) -> u16 {
    state.fpga_boot_count += core::num::Wrapping(1);
    state.fpga_rom_version = buffer[1];
    state.fpga_boot_count.0
}

/// Returns the payload contained within `buffer`.
pub fn payload(buffer: &MemBlock) -> &[u8] {
    &buffer[1..]
}

/// Returns the payload of `buffer` as a string, or None if it's not valid
/// UTF-8.
pub fn payload_as_string(buffer: &MemBlock) -> Option<&str> {
    let mut bytes = payload(buffer);
    if let Some(first_null_index) = bytes.iter().position(|&b| b == 0) {
        bytes = &bytes[..first_null_index];
    }
    core::str::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_config() {
        let mut state = ApplicationState::default();
        state.configuration.enabled_features = 0x1234;
        let mut buffer = MemBlock::with_capacity(64);
        buffer[0] = Action::ReadConfigRegisters.into();
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::ReadConfigRegisters);

        read_config_registers(&mut buffer, &state);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::OK);
        assert_eq!(buffer[1], Action::ReadConfigRegisters.into());
        assert_eq!(buffer[2], 0x34);
        assert_eq!(buffer[3], 0x12);
        assert_eq!(buffer[4], 0);
    }

    #[test]
    fn test_write_status() {
        let mut state = ApplicationState::default();
        let mut buffer = MemBlock::with_capacity(64);
        buffer[0] = Action::WriteStatusRegisters.into();
        buffer[1] = 0x12;
        buffer[2] = 0xab;
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::WriteStatusRegisters);

        write_status_registers(&buffer, &mut state);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::OK);
        assert_eq!(buffer[1], Action::WriteStatusRegisters.into());
        assert_eq!(state.status.person_status, 0xab12);
    }

    #[test]
    fn test_poll_command() {
        let mut buffer = MemBlock::with_capacity(64);
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::Poll);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::READY);
        assert_eq!(buffer[1], 0);
    }

    #[test]
    fn test_invalid_command() {
        let mut buffer = MemBlock::with_capacity(64);
        buffer[0] = 0xff;
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::Poll);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::ERROR);
        assert_eq!(buffer[1], 0xff);
    }

    #[test]
    fn test_string_from_buffer() {
        let mut buffer = MemBlock::with_capacity(64);

        buffer[1..4].clone_from_slice(b"foo");
        assert_eq!(payload_as_string(&buffer).unwrap(), "foo");

        // Invalid UTF-8
        buffer[1..4].clone_from_slice(&[255, 255, 255]);
        assert_eq!(payload_as_string(&buffer), None);
    }
}
