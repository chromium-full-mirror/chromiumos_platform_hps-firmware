// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::DebugCommand;
use crate::Error;
use crate::Result;
use application::fpga::commands;
use application::fpga::BUFFER_SIZE;
use application::Configuration;
use core::convert::TryFrom;
use embedded_hal::blocking::spi;

// Number of times the controller will poll the MCU for a response before giving
// up.
const MCU_POLL_COUNT: u32 = 1000;

const CAM_MODE_SELECT: u16 = 0x0100;

pub struct McuInterface<S> {
    spi: S,
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

    pub fn report_boot(&mut self, version: u8) -> Result<()> {
        self.clear_buffer();
        self.buffer[0] = commands::REPORT_FPGA_BOOT;
        self.buffer[1] = version;
        self.exchange()
    }

    pub fn read_config(&mut self) -> Result<Configuration> {
        self.clear_buffer();
        self.buffer[0] = commands::READ_CONFIG_REGISTERS;
        self.exchange()?;
        // safety: The transfer succeeded, so the MCU will have put a valid
        // Configuration into the returned buffer. We use the same definition of
        // Configuration, which is repr(C) and releases of the two binaries are
        // kept in sync.
        Ok(unsafe {
            core::ptr::read_unaligned(&self.buffer[2] as *const u8 as *const Configuration)
        })
    }

    pub fn write_camera_reg(&mut self, register: u16, value: u8) -> Result<()> {
        self.clear_buffer();
        self.buffer[0] = commands::WRITE_CAMERA_I2C;
        self.buffer[1] = 1;
        self.buffer[2..4].copy_from_slice(&register.to_be_bytes());
        self.buffer[4] = value;
        self.exchange()
    }

    pub fn read_camera_reg(&mut self, register: u16) -> u8 {
        loop {
            self.clear_buffer();
            self.buffer[0] = commands::READ_CAMERA_I2C;
            self.buffer[1] = 1;
            self.buffer[2..4].copy_from_slice(&register.to_be_bytes());
            if self.exchange().is_ok() {
                return self.buffer[2];
            }
        }
    }

    pub fn wait_for_camera_standby(&mut self) {
        while self.read_camera_reg(CAM_MODE_SELECT) != 0 {}
    }

    pub fn trigger_frame(&mut self) -> Result<()> {
        // TODO: Consider just making TRIGGER_FRAME into a command.
        const FRAME_COUNT: u16 = 0x3020;
        const STREAMING_N_FRAMES: u8 = 0x03;
        self.write_camera_reg(FRAME_COUNT, 1)?;
        self.write_camera_reg(CAM_MODE_SELECT, STREAMING_N_FRAMES)
    }

    /// Returns the next pending debug command (if any) together with an
    /// argument for that command.
    pub fn next_debug_command(&mut self) -> Option<(DebugCommand, u16)> {
        self.clear_buffer();
        self.buffer[0] = commands::GET_DEBUG_COMMAND;
        self.exchange().ok()?;
        Some((
            DebugCommand::try_from(self.buffer[2]).ok()?,
            u16::from_le_bytes([self.buffer[3], self.buffer[4]]),
        ))
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
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for chunk in s.as_bytes().chunks(BUFFER_SIZE - 1) {
            self.clear_buffer();
            self.buffer[0] = commands::DEBUG_LOG;
            self.buffer[1..1 + chunk.len()].copy_from_slice(chunk);
            self.exchange().map_err(|_| core::fmt::Error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::fpga::commands;
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
        let mut mcu =
            command_response_interface(vec![commands::POLL], vec![status::READY, commands::POLL]);
        mcu.resync();
        mcu.spi.done();
    }

    #[test]
    fn test_report_boot() {
        let mut mcu = command_response_interface(
            vec![commands::REPORT_FPGA_BOOT, 42],
            vec![status::OK, commands::REPORT_FPGA_BOOT],
        );
        mcu.report_boot(42).unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_println() {
        use core::fmt::Write;
        let mut command = vec![commands::DEBUG_LOG];
        command.extend_from_slice(b"Hello, world");

        let response = vec![status::OK, commands::DEBUG_LOG];

        let mut mcu = command_response_interface(command, response);

        write!(&mut mcu, "Hello, world").unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_read_config() {
        let command = vec![commands::READ_CONFIG_REGISTERS];

        let mut response = vec![status::OK, commands::READ_CONFIG_REGISTERS];
        response.extend_from_slice(&0x1234_u16.to_le_bytes());

        let mut mcu = command_response_interface(command, response);

        let config = mcu.read_config().unwrap();
        assert_eq!(config.enabled_features, 0x1234);
        mcu.spi.done();
    }

    #[test]
    fn test_write_camera_reg() {
        let command = vec![commands::WRITE_CAMERA_I2C, 1, 0x12, 0x34, 0x56];
        let response = vec![status::OK, commands::WRITE_CAMERA_I2C];
        let mut mcu = command_response_interface(command, response);
        mcu.write_camera_reg(0x1234, 0x56).unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_read_camera_reg() {
        let command = vec![commands::READ_CAMERA_I2C, 1, 0x12, 0x34];
        let response = vec![status::OK, commands::READ_CAMERA_I2C, 0x42];
        let mut mcu = command_response_interface(command, response);
        let value = mcu.read_camera_reg(0x1234);
        assert_eq!(value, 0x42);
        mcu.spi.done();
    }

    #[test]
    fn test_next_debug_command() {
        let command = vec![commands::GET_DEBUG_COMMAND];
        let response = vec![
            status::OK,
            commands::GET_DEBUG_COMMAND,
            DebugCommand::ClassificationOn.into(),
            0x34,
            0x12,
        ];
        let mut mcu = command_response_interface(command, response);
        let value = mcu.next_debug_command();
        assert_eq!(value, Some((DebugCommand::ClassificationOn, 0x1234)));
        mcu.spi.done();
    }
}
