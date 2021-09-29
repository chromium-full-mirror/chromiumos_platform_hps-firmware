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
use mcu_common::CommonHostInterface;

use crate::application_state::ApplicationState;

pub struct HostInterface<I: I2cPeripheral> {
    protocol_handler: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    Reset,
    WriteSpiFlash(WriteMemoryEvent),
    /// Try to start the FPGA if it isn't already started.
    StartFpga,
}

impl<I: I2cPeripheral> HostInterface<I> {
    pub fn new(i2c: I) -> Self {
        Self {
            protocol_handler: I2cProtocolHandler::new(i2c),
            common_interface: CommonHostInterface::new(),
        }
    }

    pub fn handle_interrupt(&mut self, state: &mut ApplicationState) -> Option<Event> {
        loop {
            match self.protocol_handler.next_event() {
                i2c_protocol::Event::ReadRegister(event) => {
                    let value = self.read_register(event.register, state);
                    event.respond(value, &mut self.protocol_handler)
                }
                i2c_protocol::Event::WriteRegister(event) => {
                    if let Some(event) = self.write_register(event.register, event.value, state) {
                        return Some(event);
                    }
                }
                i2c_protocol::Event::WriteMemory(mut event) => {
                    // We support two memory banks. One for the bitstream and
                    // one for the SOC ROM. They overlap, so you can program
                    // both at once by just writing a concatenated bistream +
                    // padding + SOC ROM to the bitstream's memory bank -
                    // however doing so is wasteful, so not recommended.
                    if event.bank == memory_banks::FPGA_BITSTREAM {
                        return Some(Event::WriteSpiFlash(event));
                    }
                    if event.bank == memory_banks::SOC_ROM {
                        event.address += mcu_common::SOC_ROM_OFFSET;
                        return Some(Event::WriteSpiFlash(event));
                    }
                }
                i2c_protocol::Event::Error(error) => {
                    self.common_interface.report_i2c_host_error(error);
                }
                i2c_protocol::Event::None => {
                    // Break out of loop.
                    return None;
                }
            }
        }
    }

    fn read_register(&self, register: u8, state: &ApplicationState) -> u16 {
        let common_value = self.common_interface.read_register(register).unwrap_or(0);
        match register {
            registers::SYSTEM_STATUS => {
                // TODO: Revisit whether APPLREADY and AVERIFY still make sense
                // to be set unconditionally once we're integrated with stage1.
                common_value | status::APPLRUN | status::APPLREADY | status::AVERIFY
            }
            registers::USER_PRESENT_STATUS => state.status.person_score as u16,
            registers::MEMORY_BANK_AVAILABLE => {
                if i2c_protocol::Buffer::is_available() {
                    (1 << memory_banks::FPGA_BITSTREAM) | (1 << memory_banks::SOC_ROM)
                } else {
                    0
                }
            }
            registers::ENABLED_FEATURES => state.status.enabled_features,
            _ => common_value,
        }
    }

    fn write_register(
        &mut self,
        register: u8,
        value: u16,
        state: &mut ApplicationState,
    ) -> Option<Event> {
        match register {
            registers::COMMAND => match value {
                commands::RESET => return Some(Event::Reset),
                _ => {}
            },
            registers::ENABLED_FEATURES => {
                state.configuration.enabled_features = value;
                // If any features are enabled, then we want to try to start the
                // FPGA.
                if value != 0 {
                    return Some(Event::StartFpga);
                }
            }
            _ => (),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake_i2c::FakeI2cPeripheral;
    use hps_interface::Hps;
    use hps_interface::HpsAdapter;

    #[test]
    fn test_system_status() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::SYSTEM_STATUS).unwrap(),
                status::OK | status::APPLRUN | status::APPLREADY | status::AVERIFY
            );
        }));
        interface.handle_interrupt(&mut state);
    }

    #[test]
    fn test_reset() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            hps.write_register(registers::COMMAND, commands::RESET)
                .unwrap();
        }));
        assert_eq!(interface.handle_interrupt(&mut state), Some(Event::Reset));
    }

    #[test]
    fn test_set_enabled_features() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            hps.write_register(registers::ENABLED_FEATURES, 1234)
                .unwrap();
        }));
        interface.handle_interrupt(&mut state);
        assert_eq!(state.configuration.enabled_features, 1234);
    }

    #[test]
    fn test_read_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_score = -55;
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::USER_PRESENT_STATUS).unwrap() as i8,
                -55
            );
        }));
        interface.handle_interrupt(&mut state);
    }
}
