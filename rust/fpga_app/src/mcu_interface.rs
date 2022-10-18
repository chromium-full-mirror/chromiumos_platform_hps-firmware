// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Microseconds;
use crate::Result;
use application::fpga::commands::Action;
use application::fpga::BUFFER_SIZE;
use application::Configuration;
use application::Status;
use embedded_hal::blocking::spi;
use mcu_common::Error;

// Number of times the controller will poll the MCU for a response before giving
// up.
const MCU_POLL_COUNT: u32 = 1000;

const CAM_MODE_SELECT: u16 = 0x0100;

/// The minimum size of packet we can send to the MCU. It's not clear why, but
/// sending less than 4 bytes causes things to break.
const MIN_BUFFER_SIZE: usize = 4;

pub struct McuInterface<S> {
    pub(crate) spi: S,
}

impl<S, E> McuInterface<S>
where
    S: spi::Transfer<u8, Error = E>,
{
    pub fn new(spi: S) -> Self {
        Self { spi }
    }

    /// Resynchronize communication with the MCU.
    pub fn resync(&mut self) {
        loop {
            let mut buffer = [0u8; MIN_BUFFER_SIZE];
            if self.exchange(&mut buffer).is_ok() {
                return;
            }
        }
    }

    pub fn report_boot(&mut self, version: u8) -> Result<()> {
        let mut buffer = [0u8; MIN_BUFFER_SIZE];
        buffer[0] = Action::ReportFpgaBoot.into();
        buffer[1] = version;
        self.exchange(&mut buffer)
    }

    pub fn read_config(&mut self) -> Result<Configuration> {
        let mut buffer = [0u8; BUFFER_SIZE];
        buffer[0] = Action::ReadConfigRegisters.into();
        self.exchange(&mut buffer[..2 + core::mem::size_of::<Configuration>()])?;
        // safety: The transfer succeeded, so the MCU will have put a valid
        // Configuration into the returned buffer. We use the same definition of
        // Configuration, which is repr(C) and releases of the two binaries are
        // kept in sync.
        Ok(unsafe { core::ptr::read_unaligned(&buffer[2] as *const u8 as *const Configuration) })
    }

    pub fn report_status(&mut self, status: &Status) -> Result<()> {
        let mut buffer = [0u8; BUFFER_SIZE];
        let status_bytes = status.to_bytes();
        buffer[0] = Action::WriteStatusRegisters.into();
        buffer[1..1 + status_bytes.len()].copy_from_slice(&status_bytes);
        self.exchange(&mut buffer[..2 + status_bytes.len()])
    }

    pub fn report_error(&mut self, error: mcu_common::Error) {
        let mut buffer = [0u8; BUFFER_SIZE];
        buffer[0] = Action::ReportFpgaError.into();
        let error_bytes = u16::from(error).to_le_bytes();
        buffer[1..1 + error_bytes.len()].copy_from_slice(&error_bytes);
        // There's not much we can do if we get an error while we're reporting
        // an error, so we just ignore it.
        let _ = self.exchange(&mut buffer[..2 + error_bytes.len()]);
    }

    pub fn report_panic(&mut self, info: &core::panic::PanicInfo) {
        use core::fmt::Write;

        let mut buffer = [0u8; BUFFER_SIZE];
        buffer[0] = Action::ReportFpgaPanic.into();
        let mut out = mcu_common::fmt::FormatOutput::new(&mut buffer[1..]);
        // There's not much we can do if we get an error while we're reporting a
        // panic, so we just ignore it.
        let _ = write!(&mut out, "{info}");
        let _ = self.exchange(&mut buffer);
    }

    pub fn write_camera_reg(&mut self, register: u16, value: u8) -> Result<()> {
        let mut buffer = [0u8; 5];
        buffer[0] = Action::WriteCameraI2c.into();
        buffer[1] = 1;
        buffer[2..4].copy_from_slice(&register.to_be_bytes());
        buffer[4] = value;
        self.exchange(&mut buffer)
    }

    pub fn read_camera_reg(&mut self, register: u16) -> u8 {
        loop {
            let mut buffer = [0u8; 4];
            buffer[0] = Action::ReadCameraI2c.into();
            buffer[1] = 1;
            buffer[2..4].copy_from_slice(&register.to_be_bytes());
            if self.exchange(&mut buffer).is_ok() {
                return buffer[2];
            }
        }
    }

    /// Sends an echo request to the MCU then checks that we get back the same data.
    pub fn echo_test(&mut self) -> Result<()> {
        let mut buffer = [0u8; BUFFER_SIZE];
        buffer[0] = Action::Echo.into();
        for (index, byte) in buffer[2..].iter_mut().enumerate() {
            *byte = index as u8;
        }
        self.exchange(&mut buffer)?;
        for (index, byte) in buffer[2..].iter_mut().enumerate() {
            if *byte != index as u8 {
                return Err(Error::FpgaMcuCommError);
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

    /// Request that the MCU trigger a single frame after the specified delay.
    pub fn trigger_frame_in(&mut self, microseconds: Microseconds) -> Result<()> {
        let mut buffer = [0u8; 5];
        buffer[0] = Action::TriggerFrame.into();
        buffer[1..5].copy_from_slice(&microseconds.0.to_le_bytes());
        self.exchange(&mut buffer)
    }

    pub fn camera(&mut self) -> hm01b0::Camera<McuInterface<S>> {
        hm01b0::Camera::new(self)
    }

    #[cfg(feature = "dev")]
    pub fn write_stdout_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        for chunk in bytes.chunks(BUFFER_SIZE - 1) {
            let mut buffer = [0u8; BUFFER_SIZE];
            buffer[0] = Action::DebugLog.into();
            buffer[1..1 + chunk.len()].copy_from_slice(chunk);
            self.exchange(&mut buffer)?;
        }
        Ok(())
    }

    /// Returns the next pending debug command (if any) together with an
    /// argument for that command.
    #[cfg(feature = "dev")]
    pub fn next_debug_command(&mut self) -> Option<(crate::DebugCommand, u16)> {
        let mut buffer = [0u8; 5];
        buffer[0] = Action::GetDebugCommand.into();
        self.exchange(&mut buffer).ok()?;
        Some((
            crate::DebugCommand::try_from(buffer[2]).ok()?,
            u16::from_le_bytes([buffer[3], buffer[4]]),
        ))
    }

    #[cfg(feature = "image-transfer")]
    pub fn send_image_data(&mut self, image_data: &[i8]) -> Result<()> {
        let mut to_send = crate::camera::START_IMAGE_MARKER
            .iter()
            .cloned()
            .chain(image_data.iter().map(|&b| b as u8))
            .chain(crate::camera::END_IMAGE_MARKER.iter().cloned())
            .peekable();
        while to_send.peek().is_some() {
            let mut buffer = [0u8; BUFFER_SIZE];
            buffer[0] = Action::DebugImage.into();
            for i in 1..BUFFER_SIZE {
                match to_send.next() {
                    Some(byte) => buffer[i] = byte,
                    None => break,
                }
            }
            self.exchange(&mut buffer)?;
        }
        Ok(())
    }

    fn exchange(&mut self, buffer: &mut [u8]) -> Result<()> {
        let command = buffer[0];
        self.spi
            .transfer(buffer)
            .map_err(|_| Error::FpgaMcuCommError)?;

        for _ in 0..MCU_POLL_COUNT {
            if buffer[0] != 0 {
                if buffer[1] == command {
                    return Ok(());
                } else {
                    return Err(Error::FpgaMcuCommError);
                }
            }
            buffer[0] = 0;
            self.spi
                .transfer(buffer)
                .map_err(|_| Error::FpgaMcuCommError)?;
        }
        Err(Error::FpgaMcuCommError)
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
                let mut buffer = [0u8; BUFFER_SIZE];
                buffer[0] = Action::DebugLog.into();
                buffer[1..1 + chunk.len()].copy_from_slice(chunk);
                self.exchange(&mut buffer).map_err(|_| core::fmt::Error)?;
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
            return Err(Error::Internal);
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
            return Err(Error::Internal);
        }
        let register = u16::from_be_bytes([bytes[0], bytes[1]]);
        buffer[0] = self.read_camera_reg(register);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::fpga::status;
    use embedded_hal_mock::spi;
    use mcu_common::Error;

    fn command_response_interface(
        mut command: Vec<u8>,
        mut response: Vec<u8>,
    ) -> McuInterface<spi::Mock> {
        assert!(command.len() <= BUFFER_SIZE);
        assert!(response.len() <= BUFFER_SIZE);
        let len = usize::max(MIN_BUFFER_SIZE, usize::max(command.len(), response.len()));
        command.resize(len, 0);
        response.resize(len, 0);
        let expectations = [
            spi::Transaction::transfer(command, vec![0u8; len]),
            // Send a couple of not-ready-yet responses.
            spi::Transaction::transfer(vec![0u8; len], vec![0u8; len]),
            spi::Transaction::transfer(vec![0u8; len], vec![0u8; len]),
            // Send our actual response.
            spi::Transaction::transfer(vec![0u8; len], response),
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
    fn test_response_to_different_command() {
        let mut mcu = command_response_interface(
            vec![Action::ReportFpgaBoot.into(), 42],
            vec![status::OK, Action::ReadConfigRegisters.into()],
        );
        assert!(mcu.report_boot(42).is_err());
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
        response.resize(2 + core::mem::size_of::<Configuration>(), 0);

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
    fn test_report_status() {
        let mut command = vec![Action::WriteStatusRegisters.into(), 0, 0, 0, 0, 0x34, 0x12];
        command.resize(2 + core::mem::size_of::<Status>(), 0);
        let mut mcu = command_response_interface(
            command,
            vec![status::OK, Action::WriteStatusRegisters.into()],
        );
        let mut status = Status::default();
        status.loop_count = 0x1234;
        mcu.report_status(&status).unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_trigger_frame_in() {
        let mut mcu = command_response_interface(
            vec![Action::TriggerFrame.into(), 0x78, 0x56, 0x34, 0x12],
            vec![status::OK, Action::TriggerFrame.into()],
        );
        mcu.trigger_frame_in(Microseconds(0x12345678)).unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_report_error() {
        let mut mcu = command_response_interface(
            vec![Action::ReportFpgaError.into(), 0x00, 0x8],
            vec![status::OK, Action::ReportFpgaError.into()],
        );
        mcu.report_error(Error::TfliteFailure);
        mcu.spi.done();
    }
}
