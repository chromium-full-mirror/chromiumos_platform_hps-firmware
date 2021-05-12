// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod commands;
mod status;

use crate::application_state::Configuration;
use crate::application_state::Status;
use crate::ApplicationState;
use mcu_common::MemBlock;

pub struct Request {
    /// An action that should be performed before we send the response. Will be
    /// absent if the request was able to be processed entirely by accessing
    /// state.
    pub action: Action,
    response_code: u8,
}

#[derive(Eq, PartialEq, Debug)]
pub enum Action {
    None,
    SetDebugLed(bool),
    ResetFpga,
    ReadConfigRegisters,
    WriteStatusRegisters,
}

impl Request {
    pub fn from_received_packet(buffer: &MemBlock) -> Self {
        let command_code = buffer[0];
        let mut action = Action::None;
        let mut response_code = status::OK;
        match command_code {
            crate::fpga::commands::POLL => {
                response_code = status::READY;
            }
            crate::fpga::commands::READ_CONFIG_REGISTERS => {
                action = Action::ReadConfigRegisters;
            }
            crate::fpga::commands::WRITE_STATUS_REGISTERS => {
                action = Action::WriteStatusRegisters;
            }
            crate::fpga::commands::SET_DEBUG_LED => {
                action = Action::SetDebugLed(buffer[1] != 0);
            }
            crate::fpga::commands::RESET_FPGA => action = Action::ResetFpga,
            _ => {
                response_code = status::ERROR;
            }
        }
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
        core::ptr::copy_nonoverlapping(
            &state.configuration,
            payload_out.as_mut_ptr() as *mut Configuration,
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
        core::ptr::copy_nonoverlapping(
            payload_in.as_ptr() as *const Status,
            &mut state.status,
            usize::min(payload_in.len(), core::mem::size_of::<Status>()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcu_common_testing::mem_block::TestingMemBlock;

    #[test]
    fn test_read_config() {
        let mut state = ApplicationState::default();
        state.configuration.enabled_features = 0x1234;
        let mut testing_block = TestingMemBlock::new();
        let mut buffer = testing_block.take().unwrap();
        buffer[0] = crate::fpga::commands::READ_CONFIG_REGISTERS;
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::ReadConfigRegisters);

        read_config_registers(&mut buffer, &state);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::OK);
        assert_eq!(buffer[1], crate::fpga::commands::READ_CONFIG_REGISTERS);
        assert_eq!(buffer[2], 0x34);
        assert_eq!(buffer[3], 0x12);
        assert_eq!(buffer[4], 0);
        testing_block.put(buffer);
    }

    #[test]
    fn test_write_status() {
        let mut state = ApplicationState::default();
        let mut testing_block = TestingMemBlock::new();
        let mut buffer = testing_block.take().unwrap();
        buffer[0] = crate::fpga::commands::WRITE_STATUS_REGISTERS;
        buffer[1] = 0x12;
        buffer[2] = 0x34;
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::WriteStatusRegisters);

        write_status_registers(&buffer, &mut state);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::OK);
        assert_eq!(buffer[1], crate::fpga::commands::WRITE_STATUS_REGISTERS);
        assert_eq!(state.status.person_score, 0x12);
        assert_eq!(state.status.not_person_score, 0x34);
        testing_block.put(buffer);
    }

    #[test]
    fn test_poll_command() {
        let mut testing_block = TestingMemBlock::new();
        let mut buffer = testing_block.take().unwrap();
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::None);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::READY);
        assert_eq!(buffer[1], 0);
        testing_block.put(buffer);
    }

    #[test]
    fn test_invalid_command() {
        let mut testing_block = TestingMemBlock::new();
        let mut buffer = testing_block.take().unwrap();
        buffer[0] = 0xff;
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::None);
        request.prepare_to_send(&mut buffer);
        assert_eq!(buffer[0], status::ERROR);
        assert_eq!(buffer[1], 0xff);
        testing_block.put(buffer);
    }

    #[test]
    fn test_fpga_reset() {
        let mut testing_block = TestingMemBlock::new();
        let mut buffer = testing_block.take().unwrap();
        buffer[0] = crate::fpga::commands::RESET_FPGA;
        let request = Request::from_received_packet(&buffer);
        assert_eq!(request.action, Action::ResetFpga);
        testing_block.put(buffer);
    }
}
