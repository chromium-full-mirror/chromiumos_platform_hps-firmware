// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod commands;

use crate::application_state::Configuration;
use crate::application_state::Status;
use crate::ApplicationState;
use mcu_common::MemBlock;

pub fn handle_fpga_packet(state: &mut ApplicationState, mut buffer: MemBlock) -> Option<MemBlock> {
    let command = buffer[0];
    match command {
        crate::fpga::commands::READ_CONFIG_REGISTERS => {
            let config_size = core::mem::size_of::<Configuration>();
            if buffer.len() >= config_size {
                buffer[0] = 0x02;
                buffer[1] = command;
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
        _ => {}
    }
    Some(buffer)
}
