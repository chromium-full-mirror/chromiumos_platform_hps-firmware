// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::CommonHostInterface;
use panic_probe as _;

pub struct HostInterface<I: I2cPeripheral> {
    protocol_handler: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
    reg9: u16,
}

impl<I: I2cPeripheral> HostInterface<I> {
    pub(crate) fn new(i2c: I) -> Self {
        Self {
            protocol_handler: I2cProtocolHandler::new(i2c),
            common_interface: CommonHostInterface::new(),
            reg9: 9,
        }
    }

    pub(crate) fn handle_interrupt(&mut self) {
        match self.protocol_handler.next_event() {
            i2c_protocol::Event::ReadRegister(event) => {
                let value = self.read_register(event.register);
                event.respond(value, &mut self.protocol_handler)
            }
            i2c_protocol::Event::WriteRegister(event) => {
                self.write_register(event.register, event.value)
            }
            i2c_protocol::Event::WriteMemory(event) => {
                // The MCU application doesn't support any writable memory banks.
                defmt::info!(
                    "Write memory bank {} @{}: {:?}",
                    event.bank,
                    event.address,
                    &*event.data
                );
            }
            i2c_protocol::Event::Error(error) => {
                defmt::error!("I2C error: {}", error.as_str());
                self.common_interface.report_i2c_host_error(error);
            }
            i2c_protocol::Event::None => return,
        }
    }

    fn read_register(&self, register: u8) -> u16 {
        if let Some(value) = self.common_interface.read_register(register) {
            return value;
        }
        match register {
            // Register 9 is a RW register that's included for now for testing
            // purposes.
            9 => self.reg9,
            _ => 0,
        }
    }

    fn write_register(&mut self, register: u8, value: u16) {
        defmt::info!("write register {}: {}", register, value);
        match register {
            9 => self.reg9 = value,
            _ => (),
        }
    }
}
