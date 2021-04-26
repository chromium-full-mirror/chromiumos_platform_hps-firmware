// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::registers;
use mcu_common::status;
use mcu_common::CommonHostInterface;

pub struct HostInterface<I: I2cPeripheral> {
    protocol_handler: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
    reg9: u16,
}

impl<I: I2cPeripheral> HostInterface<I> {
    pub fn new(i2c: I) -> Self {
        Self {
            protocol_handler: I2cProtocolHandler::new(i2c),
            common_interface: CommonHostInterface::new(),
            reg9: 9,
        }
    }

    pub fn handle_interrupt(&mut self) {
        loop {
            match self.protocol_handler.next_event() {
                i2c_protocol::Event::ReadRegister(event) => {
                    let value = self.read_register(event.register);
                    event.respond(value, &mut self.protocol_handler)
                }
                i2c_protocol::Event::WriteRegister(event) => {
                    self.write_register(event.register, event.value)
                }
                i2c_protocol::Event::WriteMemory(_event) => {
                    // The MCU application doesn't support any writable memory banks.
                }
                i2c_protocol::Event::Error(error) => {
                    self.common_interface.report_i2c_host_error(error);
                }
                i2c_protocol::Event::None => {
                    // Break out of loop.
                    return;
                }
            }
        }
    }

    fn read_register(&self, register: u8) -> u16 {
        let common_value = self.common_interface.read_register(register).unwrap_or(0);
        match register {
            registers::SYSTEM_STATUS => common_value | status::APPLRUN,
            // Register 9 is a RW register that's included for now for testing
            // purposes.
            9 => self.reg9,
            _ => common_value,
        }
    }

    fn write_register(&mut self, register: u8, value: u16) {
        match register {
            9 => self.reg9 = value,
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake_i2c::FakeI2cPeripheral;
    use hps_interface::Hps;

    #[test]
    fn test_system_status() {
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = Hps::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::SYSTEM_STATUS).unwrap(),
                status::OK | status::APPLRUN
            );
        }));
        interface.handle_interrupt();
    }
}
