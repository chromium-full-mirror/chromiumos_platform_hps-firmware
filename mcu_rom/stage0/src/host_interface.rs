// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use i2c_protocol::WriteMemoryEvent;
use mcu_common::commands;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::status;

const HARDWARE_VERSION: u16 = 1;
const RO_VERSION: u16 = 1;

pub struct HostInterface<I: I2cPeripheral> {
    i2c: I2cProtocolHandler<I>,
}

pub(crate) enum HostEvent {
    None,
    LoadApplication,
    WriteFlash(WriteMemoryEvent),
}

impl<I: I2cPeripheral> HostInterface<I> {
    pub(crate) fn new(i2c: I) -> Self {
        Self {
            i2c: I2cProtocolHandler::new(i2c),
        }
    }

    pub(crate) fn handle_i2c_events(&mut self) -> HostEvent {
        match self.i2c.next_event() {
            i2c_protocol::Event::ReadRegister(event) => {
                let result = match event.register {
                    registers::MAGIC => mcu_common::hps_magic_code(),
                    registers::HARDWARE_VERSION => (HARDWARE_VERSION << 8) | RO_VERSION,
                    registers::SYSTEM_STATUS => {
                        // Common system status
                        status::OK
                    }
                    _ => 0,
                };
                event.respond(result, &mut self.i2c);
            }
            i2c_protocol::Event::WriteRegister(event) => match event.register {
                registers::COMMAND => match event.value {
                    commands::RESET => cortex_m::peripheral::SCB::sys_reset(),
                    commands::LAUNCH => {
                        return HostEvent::LoadApplication;
                    }
                    _ => defmt::info!("Unknown command {}", event.value),
                },
                _ => defmt::info!("Write to unsupported register {}", event.register),
            },
            i2c_protocol::Event::WriteMemory(event) => match event.bank {
                memory_banks::MCU_RW => {
                    return HostEvent::WriteFlash(event);
                }
                x => {
                    defmt::warn!("Attempt to write to unsupported memory bank {}", x);
                }
            },
            i2c_protocol::Event::Error(error) => {
                defmt::error!("I2C error: {}", error.as_str())
            }
            _ => {}
        }
        HostEvent::None
    }
}
