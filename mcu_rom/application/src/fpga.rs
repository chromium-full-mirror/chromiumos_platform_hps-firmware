// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod commands;
mod status;

use crate::application_state::Configuration;
use crate::application_state::Status;
use crate::ApplicationState;
use mcu_common::MemBlock;

/// A command
pub enum Command {
    SetDebugLed(bool),
}

pub fn handle_fpga_packet(
    state: &mut ApplicationState,
    mut buffer: MemBlock,
) -> (Option<MemBlock>, Option<Command>) {
    let command_code = buffer[0];
    let mut command = None;
    let mut response_code = status::OK;
    match command_code {
        crate::fpga::commands::POLL => {
            response_code = status::READY;
        }
        crate::fpga::commands::READ_CONFIG_REGISTERS => {
            let config_size = core::mem::size_of::<Configuration>();
            if buffer.len() >= config_size {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &state.configuration,
                        buffer[2..].as_mut_ptr() as *mut Configuration,
                        config_size,
                    );
                }
            }
        }
        crate::fpga::commands::WRITE_STATUS_REGISTERS => unsafe {
            core::ptr::copy_nonoverlapping(
                buffer[1..].as_ptr() as *const Status,
                &mut state.status,
                core::mem::size_of::<Status>(),
            );
        },
        crate::fpga::commands::SET_DEBUG_LED => {
            command = Some(Command::SetDebugLed(buffer[1] != 0));
        }
        _ => {}
    }
    buffer[0] = response_code;
    buffer[1] = command_code;
    (Some(buffer), command)
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
        let response = handle_fpga_packet(&mut state, buffer).0.unwrap();
        assert_eq!(response[0], status::OK);
        assert_eq!(response[1], crate::fpga::commands::READ_CONFIG_REGISTERS);
        assert_eq!(response[2], 0x34);
        assert_eq!(response[3], 0x12);
        assert_eq!(response[4], 0);
        testing_block.put(response);
    }

    #[test]
    fn test_write_status() {
        let mut state = ApplicationState::default();
        let mut testing_block = TestingMemBlock::new();
        let mut buffer = testing_block.take().unwrap();
        buffer[0] = crate::fpga::commands::WRITE_STATUS_REGISTERS;
        buffer[1] = 0x12;
        buffer[2] = 0x34;
        let response = handle_fpga_packet(&mut state, buffer).0.unwrap();
        assert_eq!(response[0], status::OK);
        assert_eq!(response[1], crate::fpga::commands::WRITE_STATUS_REGISTERS);
        assert_eq!(state.status.person_score, 0x12);
        assert_eq!(state.status.not_person_score, 0x34);
        testing_block.put(response);
    }

    #[test]
    fn test_poll_command() {
        let mut state = ApplicationState::default();
        let mut testing_block = TestingMemBlock::new();
        let buffer = testing_block.take().unwrap();
        let response = handle_fpga_packet(&mut state, buffer).0.unwrap();
        assert_eq!(response[0], status::READY);
        assert_eq!(response[1], 0);
        testing_block.put(response);
    }
}
