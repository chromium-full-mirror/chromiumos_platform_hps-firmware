// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// ! Interface to socket communication simulating I2C behaviour

use embedded_hal::blocking::i2c;
use std::error;
use std::fmt::Display;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::Duration;

const STOP: u8 = 0x00;
const START_READ: u8 = 0x01;
const START_WRITE: u8 = 0x02;
const MAX_PACKET: usize = 65535;

#[derive(Debug)]
pub enum Error {
    SocketError(std::io::Error),
    WriteTooLarge,
    ReadTooLarge,
    WriteTimeout,
    ReadTimeout,
}

pub struct I2cOverSocket {
    socket: UnixStream,
}

impl I2cOverSocket {
    pub fn connect(file_path: &str) -> Result<I2cOverSocket, Error> {
        let socket = UnixStream::connect(file_path)?;
        socket
            .set_read_timeout(Some(Duration::new(5, 0)))
            .expect("Couldn't set read timeout");
        socket
            .set_write_timeout(Some(Duration::new(5, 0)))
            .expect("Couldn't set write timeout");
        Ok(Self { socket })
    }

    fn write_socket(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > MAX_PACKET {
            return Err(Error::WriteTooLarge);
        }

        if bytes.is_empty() {
            return Ok(());
        }

        // Send command
        self.socket.write(&[START_WRITE])?;

        // Send byte length
        self.socket.write(&(bytes.len() as u16).to_be_bytes())?;

        // Send bytes if it's WRITE
        self.socket.write(bytes)?;
        Ok(())
    }

    fn read_socket(&mut self, read_buffer: &mut [u8]) -> Result<(), Error> {
        if read_buffer.len() > MAX_PACKET {
            return Err(Error::ReadTooLarge);
        }
        if read_buffer.is_empty() {
            return Ok(());
        }

        // Send command
        self.socket.write(&[START_READ])?;

        // Send bytes to read
        self.socket
            .write(&(read_buffer.len() as u16).to_be_bytes())?;

        // Read into buffer
        self.socket.read(read_buffer)?;
        Ok(())
    }

    fn write_stop(&mut self) -> Result<(), Error> {
        self.socket.write(&[STOP])?;
        Ok(())
    }
}

impl i2c::WriteRead for I2cOverSocket {
    type Error = Error;

    fn write_read(
        &mut self,
        _address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        if !bytes.is_empty() {
            self.write_socket(bytes)?;
        }
        if !buffer.is_empty() {
            self.read_socket(buffer)?;
        }
        self.write_stop()?;
        Ok(())
    }
}

impl i2c::Write for I2cOverSocket {
    type Error = Error;

    fn write(&mut self, _addr: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.write_socket(bytes)?;
        self.write_stop()?;
        Ok(())
    }
}

impl i2c::Read for I2cOverSocket {
    type Error = Error;
    fn read(&mut self, _addr: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.read_socket(buffer)?;
        self.write_stop()?;
        Ok(())
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SocketError(inner) => write!(f, "Socket connection error:{}", inner),
            Error::WriteTooLarge => write!(f, "Write too large"),
            Error::ReadTooLarge => write!(f, "Read too large"),
            Error::ReadTimeout => write!(f, "Read timeout"),
            Error::WriteTimeout => write!(f, "Write timeout"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::SocketError(error)
    }
}

impl error::Error for Error {}
