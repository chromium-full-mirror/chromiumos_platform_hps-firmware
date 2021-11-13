// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::Result;
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
}
