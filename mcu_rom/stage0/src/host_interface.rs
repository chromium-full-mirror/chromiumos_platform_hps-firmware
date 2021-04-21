// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use i2c_peripheral::I2cPeripheral;
use i2c_protocol::Buffer;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::commands;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::CommonHostInterface;

const HARDWARE_VERSION: u16 = 1;
const RO_VERSION: u16 = 1;

pub struct HostInterface<I: I2cPeripheral> {
    i2c: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
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
        }
    }

    pub(crate) fn handle_i2c_events(&mut self) -> Option<HostEvent> {
        match self.i2c.next_event() {
            i2c_protocol::Event::ReadRegister(event) => {
                let result = self
                    .common_interface
                    .read_register(event.register)
                    .unwrap_or_else(|| match event.register {
                        registers::HARDWARE_VERSION => (HARDWARE_VERSION << 8) | RO_VERSION,
                        registers::MEMORY_BANK_AVAILABLE => {
                            if Buffer::is_available() {
                                1 << memory_banks::MCU_RW
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    });
                event.respond(result, &mut self.i2c);
            }
            i2c_protocol::Event::WriteRegister(event) => match event.register {
                registers::COMMAND => match event.value {
                    commands::RESET => return Some(HostEvent::Reset),
                    commands::LAUNCH => {
                        return Some(HostEvent::LoadApplication);
                    }
                    _ => defmt::info!("Unknown command {}", event.value),
                },
                _ => defmt::info!("Write to unsupported register {}", event.register),
            },
            i2c_protocol::Event::WriteMemory(event) => match event.bank {
                memory_banks::MCU_RW => {
                    return Some(HostEvent::WriteFlash(WriteFlashRequest {
                        address: event.address,
                        data: event.data,
                    }));
                }
                x => {
                    defmt::warn!("Attempt to write to unsupported memory bank {}", x);
                }
            },
            i2c_protocol::Event::Error(error) => {
                defmt::error!("I2C error: {}", error.as_str());
                self.common_interface.report_i2c_host_error(error);
            }
            _ => {}
        }
        None
    }

    pub(crate) fn i2c_mut(&mut self) -> &mut I {
        self.i2c.i2c_mut()
    }
}
