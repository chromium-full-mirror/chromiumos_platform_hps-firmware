// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::error;
use crate::info;
use crate::warn;
use crate::ValidStage1;
use crate::WriteProtectState;
use i2c_peripheral::I2cPeripheral;
use i2c_protocol::Buffer;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::commands;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::status;
use mcu_common::CommonHostInterface;
use mcu_common::MemBlock;

const HARDWARE_VERSION: u16 = 1;
const RO_VERSION: u16 = 1;

pub(crate) struct StageZContext {
    /// Information about stage1 or None if stage1 is absent, not validated etc.
    valid_stage1: Option<ValidStage1>,
    write_protect_state: WriteProtectState,
}

impl StageZContext {
    fn new() -> Self {
        StageZContext {
            valid_stage1: None,
            write_protect_state: WriteProtectState::Asserted,
        }
    }
    pub(crate) fn set_write_protect_state(&mut self, write_protect_state: WriteProtectState) {
        self.write_protect_state = write_protect_state;
    }
    /// Called when we've determined that stage1 is valid and jumping to it is
    /// permitted. i.e. either it had a valid signature or firmware write
    /// protect was deasserted.
    pub(crate) fn set_stage1_valid(&mut self, valid_stage1: ValidStage1) {
        self.valid_stage1 = Some(valid_stage1);
    }
    pub(crate) fn mark_stage1_invalid(&mut self) {
        self.valid_stage1 = None;
    }
    pub(crate) fn can_jump_to_stage1(&self) -> bool {
        self.valid_stage1.is_some()
    }
    fn get_verification_status(&self) -> u16 {
        if self.can_jump_to_stage1() {
            status::AVERIFY
        } else {
            status::ANONVER
        }
    }
    fn get_fw_wp_status(&self) -> u16 {
        match self.write_protect_state {
            WriteProtectState::Asserted => status::WPON,
            WriteProtectState::Deasserted => status::WPOFF,
        }
    }
    pub(crate) fn get_firmware_version(&self, register: u8) -> u16 {
        match (register, &self.valid_stage1) {
            (registers::FIRMWARE_VERSION_HIGH, Some(info)) => info.firmware_version[0],
            (registers::FIRMWARE_VERSION_LOW, Some(info)) => info.firmware_version[1],
            _ => 0xFFFF,
        }
    }
}

pub struct HostInterface<I: I2cPeripheral> {
    i2c: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
    pub(crate) stage0_ctx: StageZContext,
}

pub(crate) enum HostEvent {
    LoadApplication,
    WriteFlash(WriteFlashRequest),
    Reset,
}

pub(crate) struct WriteFlashRequest {
    pub(crate) address: u32,
    pub(crate) data: Buffer,
}

impl<I: I2cPeripheral> HostInterface<I> {
    pub(crate) fn new(i2c: I) -> Self {
        Self {
            i2c: I2cProtocolHandler::new(i2c),
            common_interface: CommonHostInterface::new(),
            stage0_ctx: StageZContext::new(),
        }
    }

    /// Supply (or return) a memory block for the purpose of receiving data.
    pub(crate) fn supply_mem_block(&mut self, mem_block: MemBlock) {
        self.i2c.supply_mem_block(mem_block);
    }

    pub(crate) fn handle_i2c_events(&mut self) -> Option<HostEvent> {
        match self.i2c.next_event() {
            Ok(i2c_protocol::Event::ReadRegister(event)) => {
                let result = self
                    .common_interface
                    .read_register(event.register)
                    .map(|common_value| match event.register {
                        registers::SYSTEM_STATUS => {
                            common_value
                                | self.stage0_ctx.get_verification_status()
                                | self.stage0_ctx.get_fw_wp_status()
                        }
                        _ => common_value,
                    })
                    .unwrap_or_else(|| match event.register {
                        registers::HARDWARE_VERSION => (HARDWARE_VERSION << 8) | RO_VERSION,
                        registers::FIRMWARE_VERSION_HIGH => self
                            .stage0_ctx
                            .get_firmware_version(registers::FIRMWARE_VERSION_HIGH),
                        registers::FIRMWARE_VERSION_LOW => self
                            .stage0_ctx
                            .get_firmware_version(registers::FIRMWARE_VERSION_LOW),
                        registers::MEMORY_BANK_AVAILABLE => {
                            if self.i2c.has_receive_buffer() {
                                1 << memory_banks::MCU_RW
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    });
                event.respond_u16(result, &mut self.i2c);
            }
            Ok(i2c_protocol::Event::WriteRegister(event)) => match event.register {
                registers::COMMAND => match event.value {
                    commands::RESET => return Some(HostEvent::Reset),
                    commands::LAUNCH1 => {
                        return Some(HostEvent::LoadApplication);
                    }
                    _ => info!("Unknown command {}", event.value),
                },
                _ => info!("Write to unsupported register {}", event.register),
            },
            Ok(i2c_protocol::Event::WriteMemory(event)) => match event.bank {
                memory_banks::MCU_RW => {
                    return Some(HostEvent::WriteFlash(WriteFlashRequest {
                        address: event.address,
                        data: event.data,
                    }));
                }
                x => {
                    warn!("Attempt to write to unsupported memory bank {}", x);
                }
            },
            Err(error) => {
                self.common_interface.report_error(error);
            }
            _ => {}
        }
        None
    }

    pub(crate) fn i2c_mut(&mut self) -> &mut I {
        self.i2c.i2c_mut()
    }

    pub(crate) fn i2c_report_err(&mut self, error: mcu_common::Error) {
        self.common_interface.report_error(error);
    }
}
