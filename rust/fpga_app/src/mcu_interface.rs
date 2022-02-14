// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::Result;
use application::fpga::commands::Action;
use application::fpga::BUFFER_SIZE;
use application::Configuration;
use application::Status;
use embedded_hal::blocking::spi;

// Number of times the controller will poll the MCU for a response before giving
// up.
const MCU_POLL_COUNT: u32 = 1000;

const CAM_MODE_SELECT: u16 = 0x0100;

pub struct McuInterface<S> {
    pub(crate) spi: S,
    buffer: [u8; BUFFER_SIZE],
}

impl<S, E> McuInterface<S>
where
    S: spi::Transfer<u8, Error = E>,
{
    pub fn new(spi: S) -> Self {
        Self {
            spi,
            buffer: [0u8; BUFFER_SIZE],
        }
    }

    /// Resynchronize communication with the MCU.
    pub fn resync(&mut self) {
        loop {
            self.clear_buffer();
            if self.exchange().is_ok() {
                return;
            }
        }
    }

    /// This allows an McuInterface to be used as an argument to mcu_println.
    pub fn mcu_mut(&mut self) -> &mut Self {
        self
    }

    pub fn report_boot(&mut self, version: u8) -> Result<()> {
        self.clear_buffer();
        self.buffer[0] = Action::ReportFpgaBoot.into();
        self.buffer[1] = version;
        self.exchange()
    }

    pub fn read_config(&mut self) -> Result<Configuration> {
        self.clear_buffer();
        self.buffer[0] = Action::ReadConfigRegisters.into();
        self.exchange()?;
        // safety: The transfer succeeded, so the MCU will have put a valid
        // Configuration into the returned buffer. We use the same definition of
        // Configuration, which is repr(C) and releases of the two binaries are
        // kept in sync.
        Ok(unsafe {
            core::ptr::read_unaligned(&self.buffer[2] as *const u8 as *const Configuration)
        })
    }

    pub fn report_status(&mut self, status: &Status) -> Result<()> {
        self.clear_buffer();
        let status_bytes = status.to_bytes();
        self.buffer[0] = Action::WriteStatusRegisters.into();
        self.buffer[1..1 + status_bytes.len()].copy_from_slice(&status_bytes);
        self.exchange()
    }

    pub fn write_camera_reg(&mut self, register: u16, value: u8) -> Result<()> {
        self.clear_buffer();
        self.buffer[0] = Action::WriteCameraI2c.into();
        self.buffer[1] = 1;
        self.buffer[2..4].copy_from_slice(&register.to_be_bytes());
        self.buffer[4] = value;
        self.exchange()
    }

    pub fn read_camera_reg(&mut self, register: u16) -> u8 {
        loop {
            self.clear_buffer();
            self.buffer[0] = Action::ReadCameraI2c.into();
            self.buffer[1] = 1;
            self.buffer[2..4].copy_from_slice(&register.to_be_bytes());
            if self.exchange().is_ok() {
                return self.buffer[2];
            }
        }
    }

    /// Sends an echo request to the MCU then checks that we get back the same data.
    pub fn echo_test(&mut self) -> Result<()> {
        self.buffer[0] = Action::Echo.into();
        for (index, byte) in self.buffer[2..].iter_mut().enumerate() {
            *byte = index as u8;
        }
        self.exchange()?;
        for (index, byte) in self.buffer[2..].iter_mut().enumerate() {
            if *byte != index as u8 {
                return Err(Error::SpiError);
            }
        }
        Ok(())
    }

    pub fn wait_for_camera_standby(&mut self) {
        while self.read_camera_reg(CAM_MODE_SELECT) != 0 {}
    }

    pub fn trigger_frame(&mut self) -> Result<()> {
        self.trigger_n_frames(1)
    }

    pub fn trigger_n_frames(&mut self, frame_count: u8) -> Result<()> {
        self.camera()
            .set_mode(hm01b0::Mode::StreamingNFrames(frame_count))?;
        Ok(())
    }

    pub fn camera(&mut self) -> hm01b0::Camera<McuInterface<S>> {
        hm01b0::Camera::new(self)
    }

    /// Returns the next pending debug command (if any) together with an
    /// argument for that command.
    #[cfg(feature = "dev")]
    pub fn next_debug_command(&mut self) -> Option<(crate::DebugCommand, u16)> {
        use core::convert::TryFrom;
        self.clear_buffer();
        self.buffer[0] = Action::GetDebugCommand.into();
        self.exchange().ok()?;
        Some((
            crate::DebugCommand::try_from(self.buffer[2]).ok()?,
            u16::from_le_bytes([self.buffer[3], self.buffer[4]]),
        ))
    }

    #[cfg(feature = "image-transfer")]
    pub fn send_image_data(&mut self, image_data: &[u8]) -> Result<()> {
        let mut to_send = crate::camera::START_IMAGE_MARKER
            .iter()
            .chain(image_data)
            .chain(crate::camera::END_IMAGE_MARKER.iter())
            .peekable();
        while to_send.peek().is_some() {
            self.clear_buffer();
            self.buffer[0] = Action::DebugImage.into();
            for i in 1..BUFFER_SIZE {
                match to_send.next() {
                    Some(byte) => self.buffer[i] = *byte,
                    None => break,
                }
            }
            self.exchange()?;
        }
        Ok(())
    }

    fn exchange(&mut self) -> Result<()> {
        self.spi
            .transfer(&mut self.buffer)
            .map_err(|_| Error::SpiError)?;

        for _ in 0..MCU_POLL_COUNT {
            if self.buffer[0] != 0 {
                return Ok(());
            }
            self.clear_buffer();
            self.spi
                .transfer(&mut self.buffer)
                .map_err(|_| Error::SpiError)?;
        }
        Err(Error::McuTimeout)
    }

    fn clear_buffer(&mut self) {
        for byte in self.buffer.iter_mut() {
            *byte = 0;
        }
    }
}

impl<S, E> core::fmt::Write for McuInterface<S>
where
    S: spi::Transfer<u8, Error = E>,
{
    fn write_str(&mut self, _s: &str) -> core::fmt::Result {
        #[cfg(feature = "dev")]
        {
            for chunk in _s.as_bytes().chunks(BUFFER_SIZE - 1) {
                self.clear_buffer();
                self.buffer[0] = Action::DebugLog.into();
                self.buffer[1..1 + chunk.len()].copy_from_slice(chunk);
                self.exchange().map_err(|_| core::fmt::Error)?;
            }
        }
        Ok(())
    }
}

// The following two implementations for I2C are to allow `McuInterface::camera`
// to work. They're not intended for use elsewhere.

impl<S> embedded_hal::blocking::i2c::Write for McuInterface<S>
where
    S: spi::Transfer<u8>,
{
    type Error = crate::Error;

    fn write(&mut self, _address: u8, bytes: &[u8]) -> Result<()> {
        if bytes.len() != 3 {
            return Err(Error::InvalidArg);
        }
        let register = u16::from_be_bytes([bytes[0], bytes[1]]);
        let value = bytes[2];
        self.write_camera_reg(register, value)
    }
}

impl<S> embedded_hal::blocking::i2c::WriteRead for McuInterface<S>
where
    S: spi::Transfer<u8>,
{
    type Error = crate::Error;

    fn write_read(&mut self, _address: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<()> {
        if bytes.len() != 2 || buffer.len() != 1 {
            return Err(Error::InvalidArg);
        }
        let register = u16::from_be_bytes([bytes[0], bytes[1]]);
        buffer[0] = self.read_camera_reg(register);
        Ok(())
    }
}

impl From<hm01b0::Error> for Error {
    fn from(_: hm01b0::Error) -> Self {
        Self::Camera
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::fpga::status;
    use embedded_hal_mock::spi;

    fn command_response_interface(
        mut command: Vec<u8>,
        mut response: Vec<u8>,
    ) -> McuInterface<spi::Mock> {
        assert!(command.len() <= BUFFER_SIZE);
        assert!(response.len() <= BUFFER_SIZE);
        command.resize(BUFFER_SIZE, 0);
        response.resize(BUFFER_SIZE, 0);
        let expectations = [
            spi::Transaction::transfer(command, vec![0u8; BUFFER_SIZE]),
            // Send a couple of not-ready-yet responses.
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            // Send our actual response.
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], response),
        ];

        let spi = spi::Mock::new(&expectations);
        McuInterface::new(spi)
    }

    #[test]
    fn test_resync() {
        let mut mcu = command_response_interface(
            vec![Action::Poll.into()],
            vec![status::READY, Action::Poll.into()],
        );
        mcu.resync();
        mcu.spi.done();
    }

    #[test]
    fn test_report_boot() {
        let mut mcu = command_response_interface(
            vec![Action::ReportFpgaBoot.into(), 42],
            vec![status::OK, Action::ReportFpgaBoot.into()],
        );
        mcu.report_boot(42).unwrap();
        mcu.spi.done();
    }

    #[test]
    #[cfg(feature = "dev")]
    fn test_println() {
        use core::fmt::Write;
        let mut command = vec![Action::DebugLog.into()];
        command.extend_from_slice(b"Hello, world");

        let response = vec![status::OK, Action::DebugLog.into()];

        let mut mcu = command_response_interface(command, response);

        write!(&mut mcu, "Hello, world").unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_read_config() {
        let command = vec![Action::ReadConfigRegisters.into()];

        let mut response = vec![status::OK, Action::ReadConfigRegisters.into()];
        response.extend_from_slice(&0x1234_u16.to_le_bytes());

        let mut mcu = command_response_interface(command, response);

        let config = mcu.read_config().unwrap();
        assert_eq!(config.enabled_features, 0x1234);
        mcu.spi.done();
    }

    #[test]
    fn test_write_camera_reg() {
        let command = vec![Action::WriteCameraI2c.into(), 1, 0x12, 0x34, 0x56];
        let response = vec![status::OK, Action::WriteCameraI2c.into()];
        let mut mcu = command_response_interface(command, response);
        mcu.write_camera_reg(0x1234, 0x56).unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_read_camera_reg() {
        let command = vec![Action::ReadCameraI2c.into(), 1, 0x12, 0x34];
        let response = vec![status::OK, Action::ReadCameraI2c.into(), 0x42];
        let mut mcu = command_response_interface(command, response);
        let value = mcu.read_camera_reg(0x1234);
        assert_eq!(value, 0x42);
        mcu.spi.done();
    }

    #[test]
    #[cfg(feature = "dev")]
    fn test_next_debug_command() {
        let command = vec![Action::GetDebugCommand.into()];
        let response = vec![
            status::OK,
            Action::GetDebugCommand.into(),
            crate::DebugCommand::ClassificationOn.into(),
            0x34,
            0x12,
        ];
        let mut mcu = command_response_interface(command, response);
        let value = mcu.next_debug_command();
        assert_eq!(value, Some((crate::DebugCommand::ClassificationOn, 0x1234)));
        mcu.spi.done();
    }

    #[test]
    fn test_report_status() {
        let mut mcu = command_response_interface(
            vec![Action::WriteStatusRegisters.into(), 0, 0, 0, 0, 0x34, 0x12],
            vec![status::OK, Action::WriteStatusRegisters.into()],
        );
        let mut status = Status::default();
        status.loop_count = 0x1234;
        mcu.report_status(&status).unwrap();
        mcu.spi.done();
    }
}
