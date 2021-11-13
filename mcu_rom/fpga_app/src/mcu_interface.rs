// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::Result;
use application::fpga::commands;
use application::fpga::BUFFER_SIZE;
use embedded_hal::blocking::spi;

// Number of times the controller will poll the MCU for a response before giving
// up.
const MCU_POLL_COUNT: u32 = 1000;

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

    #[test]
    fn test_resync() {
        let mut command = vec![commands::POLL];
        command.resize(BUFFER_SIZE, 0);

        let mut response = vec![status::READY, commands::POLL];
        response.resize(BUFFER_SIZE, 0);

        let expectations = [
            spi::Transaction::transfer(command, vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], response),
        ];

        let spi = spi::Mock::new(&expectations);
        let mut mcu = McuInterface::new(spi);
        mcu.resync();
        mcu.spi.done();
    }

    #[test]
    fn test_report_boot() {
        let mut command = vec![commands::REPORT_FPGA_BOOT, 42];
        command.resize(BUFFER_SIZE, 0);

        let mut response = vec![status::OK, commands::REPORT_FPGA_BOOT];
        response.resize(BUFFER_SIZE, 0);

        let expectations = [
            spi::Transaction::transfer(command, vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], response),
        ];

        let spi = spi::Mock::new(&expectations);
        let mut mcu = McuInterface::new(spi);
        mcu.report_boot(42).unwrap();
        mcu.spi.done();
    }

    #[test]
    fn test_println() {
        use core::fmt::Write;
        let mut command = vec![commands::DEBUG_LOG];
        command.extend_from_slice(b"Hello, world");
        command.resize(BUFFER_SIZE, 0);

        let mut response = vec![status::OK, commands::DEBUG_LOG];
        response.resize(BUFFER_SIZE, 0);

        let expectations = [
            spi::Transaction::transfer(command, vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], vec![0u8; BUFFER_SIZE]),
            spi::Transaction::transfer(vec![0u8; BUFFER_SIZE], response),
        ];

        let spi = spi::Mock::new(&expectations);
        let mut mcu = McuInterface::new(spi);
        write!(&mut mcu, "Hello, world").unwrap();
        mcu.spi.done();
    }
}
