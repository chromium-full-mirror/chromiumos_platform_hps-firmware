// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use i2c_protocol::ReadRegisterEvent;
use i2c_protocol::WriteMemoryEvent;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::CommonHostInterface;
use mcu_common::MemBlock;
use mcu_common::Status;

use crate::application_state::ApplicationState;
use crate::trace::Subsystem;
use crate::trace::Trace;

pub struct HostInterface<I: I2cPeripheral> {
    protocol_handler: I2cProtocolHandler<I>,
    common_interface: CommonHostInterface,
    pub spi_flash_status: u16,
    trace: Trace,
    #[cfg(feature = "image-transfer")]
    outgoing_image_data: Option<i2c_protocol::Buffer>,
    application_running: bool,
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
            spi_flash_status: 0xdead,
            trace: Trace::new(),
            #[cfg(feature = "image-transfer")]
            outgoing_image_data: None,
            application_running: false,
        }
    }

    pub fn trace(&mut self, ss: Subsystem, device: u8, value: u16) {
        self.trace.trace(ss, device, value)
    }

    /// Supply (or return) a memory block for the purpose of receiving data.
    pub fn supply_mem_block(&mut self, mem_block: MemBlock) {
        self.protocol_handler.supply_mem_block(mem_block);
    }

    pub fn handle_interrupt(&mut self, state: &mut ApplicationState) -> Option<Event> {
        loop {
            match self.protocol_handler.next_event() {
                Ok(i2c_protocol::Event::ReadRegister(event)) => {
                    self.read_register(event, state);
                }
                Ok(i2c_protocol::Event::WriteRegister(event)) => {
                    if let Some(event) = self.write_register(event.register, event.value, state) {
                        return Some(event);
                    }
                }
                Ok(i2c_protocol::Event::WriteMemory(mut event)) => {
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
                Err(error) => {
                    self.common_interface.report_error(error);
                }
                Ok(i2c_protocol::Event::None) => {
                    // Break out of loop.
                    return None;
                }
            }
        }
    }

    pub fn report_error(&mut self, error: mcu_common::Error) {
        self.common_interface.report_error(error);
    }

    fn read_register(&mut self, event: ReadRegisterEvent, state: &ApplicationState) {
        let common_value = self
            .common_interface
            .read_register(event.register)
            .unwrap_or(0);
        let result = match event.register {
            registers::SYSTEM_STATUS => {
                // TODO: Consider unsetting AVERIFY. It's a stage0 bit.
                common_value | (self.stage_status() | Status::AVERIFY).bits()
            }
            registers::USER_PRESENT_STATUS => state.status.person_status,
            registers::SECOND_PERSON_STATUS => state.status.second_person_status,
            registers::MEMORY_BANK_AVAILABLE => {
                if self.protocol_handler.has_receive_buffer() {
                    (1 << memory_banks::FPGA_BITSTREAM) | (1 << memory_banks::SOC_ROM)
                } else {
                    0
                }
            }
            registers::ENABLED_FEATURES => state.status.enabled_features,
            registers::FPGA_BOOT_COUNT => state.fpga_boot_count.0,
            registers::FPGA_LOOP_COUNT => state.status.loop_count,
            registers::FPGA_ROM_VERSION => state.fpga_rom_version as u16,
            registers::SPI_FLASH_STATUS => self.spi_flash_status,
            registers::DEBUG_IDX_REG => self.trace.get_idx(),
            registers::DEBUG_VAL_REG => self.trace.get_val(),
            #[cfg(feature = "image-transfer")]
            registers::IMAGE_DATA_AVAILABLE => {
                if self.outgoing_image_data.is_some() {
                    1
                } else {
                    0
                }
            }
            #[cfg(feature = "image-transfer")]
            registers::IMAGE_DATA => {
                if let Some(data) = self.outgoing_image_data.take() {
                    event.respond(data, &mut self.protocol_handler);
                    return;
                }
                0
            }
            _ => common_value,
        };
        event.respond_u16(result, &mut self.protocol_handler);
    }

    fn write_register(
        &mut self,
        register: u8,
        value: u16,
        state: &mut ApplicationState,
    ) -> Option<Event> {
        match register {
            registers::COMMAND => match Command::try_from(value) {
                Ok(Command::Reset) => return Some(Event::Reset),
                Ok(Command::LaunchApp) => return Some(Event::StartFpga),
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

    pub fn set_application_running(&mut self, running: bool) {
        self.application_running = running;
    }

    fn stage_status(&self) -> Status {
        if self.application_running {
            Status::APPLREADY
        } else {
            Status::STAGE1
        }
    }

    #[cfg(feature = "image-transfer")]
    pub fn queue_image_data_for_i2c(&mut self, data: &[u8]) -> bool {
        if let Some(i2c_buffer) = self.protocol_handler.take_mem_block() {
            self.outgoing_image_data = Some(i2c_protocol::Buffer::copied_from(i2c_buffer, data));
            true
        } else {
            false
        }
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
                (Status::OK | Status::STAGE1 | Status::AVERIFY).bits()
            );
        }));
        interface.handle_interrupt(&mut state);
    }

    #[test]
    fn test_reset() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            hps.perform_command(Command::Reset).unwrap();
        }));
        assert_eq!(interface.handle_interrupt(&mut state), Some(Event::Reset));
    }

    #[test]
    fn test_launch_app() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            hps.perform_command(Command::LaunchApp).unwrap();
        }));
        assert_eq!(
            interface.handle_interrupt(&mut state),
            Some(Event::StartFpga)
        );
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
    fn test_image_usable_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_status = 0x8055;
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::USER_PRESENT_STATUS).unwrap() as u16,
                0x8055
            );
        }));
        interface.handle_interrupt(&mut state);
    }
    #[test]
    fn test_image_usable_no_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_status = 0x80AB;
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::USER_PRESENT_STATUS).unwrap() as u16,
                0x80AB
            );
        }));
        interface.handle_interrupt(&mut state);
    }
    #[test]
    fn test_image_unusable_no_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_status = 0x00AB;
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::USER_PRESENT_STATUS).unwrap() as u16,
                0x00AB
            );
        }));
        interface.handle_interrupt(&mut state);
    }

    #[test]
    fn test_second_person() {
        let mut state = ApplicationState::default();
        state.status.second_person_status = 0x807A;
        let mut interface = HostInterface::new(FakeI2cPeripheral::new(|device| {
            let mut hps = HpsAdapter::without_magic_check(device);
            assert_eq!(
                hps.read_register(registers::SECOND_PERSON_STATUS).unwrap() as u16,
                0x807A
            );
        }));
        interface.handle_interrupt(&mut state);
    }
}
