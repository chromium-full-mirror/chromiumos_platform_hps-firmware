// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use embedded_hal::digital::v2::OutputPin;
use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use i2c_protocol::ReadRegisterEvent;
use i2c_protocol::WriteMemoryEvent;
use log::error;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::registers::Register;
use mcu_common::Buffer;
use mcu_common::CommonHostInterface;
use mcu_common::Error;
use mcu_common::ImageHeader;
use mcu_common::MemBlock;
use mcu_common::PartIds;
use mcu_common::Status;

use crate::application_state::ApplicationState;

pub struct HostInterface<I: I2cPeripheral, N: OutputPin> {
    protocol_handler: I2cProtocolHandler<I>,
    interrupt: N,
    common_interface: CommonHostInterface,
    pub spi_flash_status: u16,
    #[cfg(feature = "image-transfer")]
    outgoing_image_data: Option<mcu_common::Buffer>,
    application_running: bool,
    active_command_count: u8,
    spi_flash_erasing: bool,
    pub init_status: Status,
    pub part_ids: PartIds,
    image_header: &'static ImageHeader,
    previous_crash: Option<mcu_common::Buffer>,
    fpga_crash: Option<mcu_common::Buffer>,
    #[cfg(feature = "dev")]
    pub debug: DebugValues,
}

#[cfg(feature = "dev")]
#[derive(Default)]
pub struct DebugValues {
    pub d1: u16,
    pub d2: u16,
    pub d3: u16,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    WriteSpiFlash(WriteMemoryEvent),
    Command(Command),
    TestCameraI2c,
}

impl<I: I2cPeripheral, N: OutputPin> HostInterface<I, N> {
    pub fn new(i2c: I, interrupt: N, image_header: &'static ImageHeader) -> Self {
        Self {
            protocol_handler: I2cProtocolHandler::new(i2c),
            interrupt: interrupt,
            common_interface: CommonHostInterface::new(),
            spi_flash_status: 0xdead,
            #[cfg(feature = "image-transfer")]
            outgoing_image_data: None,
            application_running: false,
            active_command_count: 0,
            spi_flash_erasing: false,
            init_status: Status::empty(),
            part_ids: PartIds::default(),
            image_header,
            previous_crash: None,
            fpga_crash: None,
            #[cfg(feature = "dev")]
            debug: DebugValues::default(),
        }
    }

    pub fn reset_bus(&mut self) {
        self.protocol_handler.i2c_mut().reset();
    }

    pub fn i2c_mut(&mut self) -> &mut I {
        self.protocol_handler.i2c_mut()
    }

    pub fn assert_interrupt(&mut self) {
        let _ = self.interrupt.set_low();
    }

    pub fn deassert_interrupt(&mut self) {
        let _ = self.interrupt.set_high();
    }

    /// Supply (or return) a memory block for the purpose of receiving data.
    pub fn supply_mem_block(&mut self, mem_block: MemBlock) {
        self.protocol_handler.supply_mem_block(mem_block);
    }

    pub fn handle_i2c_events(&mut self, state: &mut ApplicationState) -> Option<Event> {
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
                    match event.bank {
                        memory_banks::FPGA_BITSTREAM => {
                            return Some(Event::WriteSpiFlash(event));
                        }
                        memory_banks::SOC_ROM => {
                            event.address += mcu_common::SOC_ROM_OFFSET;
                            return Some(Event::WriteSpiFlash(event));
                        }
                        _ => {
                            self.common_interface.report_error(Error::HostI2cBadRequest);
                        }
                    };
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

    pub fn report_status_update(&mut self, _state: &ApplicationState) {
        // TODO(dcallagh): only send useful/wanted interrupts to avoid waking the AP unnecessarily.
        self.assert_interrupt();
    }

    pub fn report_error(&mut self, error: mcu_common::Error) {
        self.common_interface.report_error(error);
    }

    pub fn report_fpga_panic(&mut self, mut bytes: &[u8]) {
        if let Some(end) = bytes.iter().position(|b| *b == 0) {
            bytes = &bytes[..end];
        }
        if let Ok(message) = core::str::from_utf8(bytes) {
            error!("FPGA reported panic: {message}");
        }
        self.report_error(mcu_common::Error::FpgaPanic);
        if let Some(mem_block) = self.protocol_handler.take_mem_block() {
            self.fpga_crash = Some(Buffer::copied_from(mem_block, bytes));
        }
    }

    pub fn report_previous_crash(&mut self, buffer: Buffer) {
        self.previous_crash = Some(buffer);
        self.report_error(mcu_common::Error::Panic);
    }

    fn read_register(&mut self, event: ReadRegisterEvent, state: &ApplicationState) {
        let common_value = self
            .common_interface
            .read_register(event.register)
            .unwrap_or(0);
        let result = match event.register {
            Register::SystemStatus => common_value | self.status().bits(),
            Register::UserPresentStatus => {
                self.deassert_interrupt();
                state.status.person_status
            }
            Register::SecondPersonStatus => {
                self.deassert_interrupt();
                state.status.second_person_status
            }
            Register::MemoryBankAvailable => {
                if !self.spi_flash_erasing && self.protocol_handler.has_receive_buffer() {
                    (1 << memory_banks::FPGA_BITSTREAM) | (1 << memory_banks::SOC_ROM)
                } else {
                    0
                }
            }
            Register::EnabledFeatures => state.status.enabled_features,
            Register::FpgaBootCount => state.fpga_boot_count.0,
            Register::FpgaLoopCount => state.status.loop_count,
            Register::FpgaRomVersion => state.fpga_rom_version as u16,
            Register::SpiFlashStatus => self.spi_flash_status,
            Register::PartIds => {
                if let Some(mem_block) = self.protocol_handler.take_mem_block() {
                    let mut buffer = Buffer::new(mem_block);
                    if self.part_ids.write_to_buffer(&mut buffer).is_ok() {
                        event.respond(buffer, &mut self.protocol_handler);
                        return;
                    }
                }
                // If the memory block is unavailable, say because there's a SPI
                // flash write in progress, then we return 0.
                0
            }
            #[cfg(feature = "image-transfer")]
            Register::ImageDataAvailable => {
                if self.outgoing_image_data.is_some() {
                    1
                } else {
                    0
                }
            }
            #[cfg(feature = "image-transfer")]
            Register::ImageData => {
                if let Some(data) = self.outgoing_image_data.take() {
                    event.respond(data, &mut self.protocol_handler);
                    return;
                }
                0
            }
            Register::CameraTestIterations => state.camera_test_iterations,
            Register::FirmwareVersionHigh => self.get_version_part(0),
            Register::FirmwareVersionLow => self.get_version_part(2),
            Register::PreviousCrash => {
                if let Some(report) = self.previous_crash.take() {
                    event.respond(report, &mut self.protocol_handler);
                    return;
                }
                0
            }
            Register::FpgaCrash => {
                if let Some(report) = self.fpga_crash.take() {
                    event.respond(report, &mut self.protocol_handler);
                    return;
                }
                0
            }
            #[cfg(feature = "dev")]
            Register::Debug1 => self.debug.d1,
            #[cfg(feature = "dev")]
            Register::Debug2 => self.debug.d2,
            #[cfg(feature = "dev")]
            Register::Debug3 => self.debug.d3,
            _ => common_value,
        };
        event.respond_u16(result, &mut self.protocol_handler);
    }

    fn write_register(
        &mut self,
        register: Register,
        value: u16,
        state: &mut ApplicationState,
    ) -> Option<Event> {
        match register {
            Register::Command => {
                if let Ok(command) = Command::try_from(value) {
                    self.command_triggered();
                    return Some(Event::Command(command));
                } else {
                    self.report_error(Error::HostI2cBadRequest);
                }
            }
            Register::EnabledFeatures => {
                state.configuration.enabled_features = value;
            }
            Register::CameraConfig => {
                state.configuration.camera_config = value;
            }
            Register::CameraTestIterations => {
                state.camera_test_iterations = value;
                return Some(Event::TestCameraI2c);
            }
            #[cfg(feature = "dev")]
            Register::Debug1 => self.debug.d1 = value,
            #[cfg(feature = "dev")]
            Register::Debug2 => self.debug.d2 = value,
            #[cfg(feature = "dev")]
            Register::Debug3 => self.debug.d3 = value,
            _ => self.report_error(Error::HostI2cBadRequest),
        }
        None
    }

    pub fn command_triggered(&mut self) {
        self.active_command_count += 1;
    }

    pub fn command_completed(&mut self) {
        self.active_command_count -= 1;
    }

    pub fn set_application_running(&mut self, running: bool) {
        self.application_running = running;
    }

    fn status(&self) -> Status {
        let mut status = self.stage_status();
        if self.active_command_count > 0 {
            status |= Status::COMMAND_IN_PROGRESS;
        }
        status | self.init_status
    }

    fn stage_status(&self) -> Status {
        if self.application_running {
            Status::APPLREADY
        } else {
            Status::STAGE1
        }
    }

    pub fn set_spi_flash_erasing(&mut self, erasing: bool) {
        self.spi_flash_erasing = erasing;
    }

    #[cfg(feature = "image-transfer")]
    pub fn queue_image_data_for_i2c(&mut self, data: &[u8]) -> bool {
        if let Some(i2c_buffer) = self.protocol_handler.take_mem_block() {
            self.outgoing_image_data = Some(mcu_common::Buffer::copied_from(i2c_buffer, data));
            true
        } else {
            false
        }
    }

    fn get_version_part(&self, offset: usize) -> u16 {
        u16::from_be_bytes([
            self.image_header.sig.raw_bytes[offset],
            self.image_header.sig.raw_bytes[offset + 1],
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake_i2c::FakeI2cPeripheral;
    use hps_interface::Hps;
    use hps_interface::HpsAdapter;
    use hps_interface::NoopInterruptLine;

    struct FakePin {}
    impl OutputPin for FakePin {
        type Error = std::convert::Infallible;
        fn set_low(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    static HEADER: ImageHeader = create_test_header();

    #[test]
    fn test_read_register_twice() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                hps.check_magic().unwrap();
                hps.check_magic().unwrap();
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    #[test]
    fn test_write_nonexistent_memory_bank() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                hps.write_memory(3, 0, &[0xaa; 64]).unwrap();
            }),
            FakePin {},
            &HEADER,
        );
        interface.supply_mem_block(MemBlock::with_capacity(64));
        interface.handle_i2c_events(&mut state);
        assert_eq!(
            interface.common_interface.get_error(),
            Error::HostI2cBadRequest
        );
    }

    #[test]
    fn test_system_status() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                assert_eq!(
                    hps.read_register(Register::SystemStatus).unwrap(),
                    (Status::OK | Status::STAGE1).bits()
                );
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    #[test]
    fn test_reset() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                hps.perform_command(Command::Reset).unwrap();
            }),
            FakePin {},
            &HEADER,
        );
        assert_eq!(
            interface.handle_i2c_events(&mut state),
            Some(Event::Command(Command::Reset))
        );
    }

    #[test]
    fn test_launch_app() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                hps.perform_command(Command::LaunchApp).unwrap();
            }),
            FakePin {},
            &HEADER,
        );
        assert_eq!(
            interface.handle_i2c_events(&mut state),
            Some(Event::Command(Command::LaunchApp))
        );
    }

    #[test]
    fn test_set_enabled_features() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                hps.write_register(Register::EnabledFeatures, 1234).unwrap();
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
        assert_eq!(state.configuration.enabled_features, 1234);
    }

    #[test]
    fn test_image_usable_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_status = 0x8055;
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                assert_eq!(
                    hps.read_register(Register::UserPresentStatus).unwrap() as u16,
                    0x8055
                );
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    #[test]
    fn test_image_usable_no_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_status = 0x80AB;
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                assert_eq!(
                    hps.read_register(Register::UserPresentStatus).unwrap() as u16,
                    0x80AB
                );
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    #[test]
    fn test_image_unusable_no_user_present() {
        let mut state = ApplicationState::default();
        state.status.person_status = 0x00AB;
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                assert_eq!(
                    hps.read_register(Register::UserPresentStatus).unwrap() as u16,
                    0x00AB
                );
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    #[test]
    fn test_second_person() {
        let mut state = ApplicationState::default();
        state.status.second_person_status = 0x807A;
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                assert_eq!(
                    hps.read_register(Register::SecondPersonStatus).unwrap() as u16,
                    0x807A
                );
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    #[test]
    fn test_firmware_version() {
        let mut state = ApplicationState::default();
        let mut interface = HostInterface::new(
            FakeI2cPeripheral::new(|device| {
                let mut hps = HpsAdapter::without_magic_check(
                    Box::new(device),
                    Box::new(NoopInterruptLine {}),
                );
                assert_eq!(
                    hps.read_register(Register::FirmwareVersionLow).unwrap() as u16,
                    0x0304
                );
                assert_eq!(
                    hps.read_register(Register::FirmwareVersionHigh).unwrap() as u16,
                    0x0102
                );
            }),
            FakePin {},
            &HEADER,
        );
        interface.handle_i2c_events(&mut state);
    }

    const fn create_test_header() -> ImageHeader {
        let mut header = ImageHeader::empty();
        header.sig.raw_bytes[0] = 1;
        header.sig.raw_bytes[1] = 2;
        header.sig.raw_bytes[2] = 3;
        header.sig.raw_bytes[3] = 4;
        header
    }
}
