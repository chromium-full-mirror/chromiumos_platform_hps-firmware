// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use embedded_hal::blocking::i2c;
use serialport::Error;
use serialport::SerialPort;
use std::time::Duration;

pub struct I2cOverUart {
    port: Box<dyn SerialPort>,
}

fn error(msg: &str) -> Error {
    Error {
        kind: serialport::ErrorKind::Unknown,
        description: msg.into(),
    }
}

impl I2cOverUart {
    pub fn new(uart_path: &str) -> Result<I2cOverUart, Error> {
        let port = serialport::new(uart_path, 115_200)
            .timeout(Duration::from_millis(1000))
            .open()?;

        Ok(Self { port })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() >= 0x80 {
            return Err(error("Write buffer too long"));
        }
        self.port.write(&[0x80 | (bytes.len() as u8)])?;
        self.port.write(bytes)?;
        Ok(())
    }
}

impl i2c::WriteRead for I2cOverUart {
    type Error = Error;

    fn write_read(
        &mut self,
        _address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        if !bytes.is_empty() {
            self.write(bytes)?;
        }
        if !buffer.is_empty() {
            if buffer.len() >= 0x80 {
                return Err(error("Read buffer too long"));
            }
            self.port.write(&[buffer.len() as u8])?;
            self.port.read(buffer)?;
        }
        // Stop
        self.write(&[0])?;
        Ok(())
    }
}

impl i2c::Write for I2cOverUart {
    type Error = Error;

    fn write(&mut self, _addr: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.write(bytes)
    }
}
