// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use libstage0::WriteProtectState;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::registers::Register;
use mcu_common::Buffer;
use mcu_common::CommonHostInterface;
use mcu_common::Error;
use mcu_common::MemBlock;
use mcu_common::Status;

pub struct HostInterface<I: I2cPeripheral> {
    i2c: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
    write_protect_state: WriteProtectState,
}

pub(crate) enum HostEvent {
    WriteFlash(WriteFlashRequest),
    Command(Command),
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
            write_protect_state: WriteProtectState::Asserted,
        }
    }

    pub(crate) fn set_write_protect_state(&mut self, write_protect_state: WriteProtectState) {
        self.write_protect_state = write_protect_state;
    }

    fn get_fw_wp_status(&self) -> Status {
        match self.write_protect_state {
            WriteProtectState::Asserted => Status::WPON,
            WriteProtectState::Deasserted => Status::WPOFF,
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
                        Register::SystemStatus => {
                            common_value | (Status::STAGE0 | self.get_fw_wp_status()).bits()
                        }
                        _ => common_value,
                    })
                    .unwrap_or_else(|| match event.register {
                        Register::HardwareVersion => libstage0::COMBINED_VERSION,
                        Register::MemoryBankAvailable => {
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
                Register::Command => {
                    if let Ok(command) = Command::try_from(event.value) {
                        return Some(HostEvent::Command(command));
                    } else {
                        self.common_interface.report_error(Error::HostI2cBadRequest);
                    }
                }
                _ => self.common_interface.report_error(Error::HostI2cBadRequest),
            },
            Ok(i2c_protocol::Event::WriteMemory(event)) => match event.bank {
                memory_banks::MCU_RW => {
                    return Some(HostEvent::WriteFlash(WriteFlashRequest {
                        address: event.address,
                        data: event.data,
                    }));
                }
                _ => self.common_interface.report_error(Error::HostI2cBadRequest),
            },
            Err(error) => {
                self.common_interface.report_error(error);
            }
            _ => {}
        }
        None
    }

    pub(crate) fn write_protect_state(&self) -> WriteProtectState {
        self.write_protect_state
    }

    pub(crate) fn get_error(&self) -> mcu_common::Error {
        self.common_interface.get_error()
    }

    pub(crate) fn i2c_mut(&mut self) -> &mut I {
        self.i2c.i2c_mut()
    }

    pub(crate) fn i2c_report_err(&mut self, error: mcu_common::Error) {
        self.common_interface.report_error(error);
    }
}
