// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use i2c_peripheral::I2cError;
use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;
use std::fmt::Display;
use std::sync::mpsc;
use std::thread::JoinHandle;

#[derive(Debug)]
pub enum Error {
    Disconnected,
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A fake I2C bus as seen from the peripheral side.
pub struct FakeI2cPeripheral {
    // Only `None` while dropping.
    events_from_host: Option<mpsc::Receiver<nb::Result<I2cEvent, I2cError>>>,
    // Only `None` while dropping.
    bytes_to_host: Option<mpsc::Sender<u8>>,
    // This is optional so that we can release ownership when we join in drop.
    host_thread: Option<JoinHandle<()>>,
}

/// A fake I2C bus as seen from the controller side.
pub struct FakeI2cController {
    events_to_device: mpsc::Sender<nb::Result<I2cEvent, I2cError>>,
    bytes_from_device: mpsc::Receiver<u8>,
}

impl FakeI2cPeripheral {
    /// Runs `host_code` in a new thread, passing it a fake I2C controller.
    /// Returns a fake I2C peripheral which represents the other end of the
    /// connection.
    pub fn new(host_code: impl FnOnce(FakeI2cController) + Send + 'static) -> FakeI2cPeripheral {
        let (events_to_device, events_from_host) = mpsc::channel();
        let (bytes_to_host, bytes_from_device) = mpsc::channel();
        let host = FakeI2cController {
            events_to_device,
            bytes_from_device,
        };
        let host_thread = std::thread::spawn(move || host_code(host));
        FakeI2cPeripheral {
            events_from_host: Some(events_from_host),
            bytes_to_host: Some(bytes_to_host),
            host_thread: Some(host_thread),
        }
    }
}

impl Drop for FakeI2cPeripheral {
    fn drop(&mut self) {
        // Before we join the host thread, we need to close the channel that it
        // might be waiting on, otherwise our test might not terminate.
        self.events_from_host.take();
        self.bytes_to_host.take();
        self.host_thread
            .take()
            .unwrap()
            .join()
            .expect("Host I2C thread failed");
    }
}

impl WriteRead for FakeI2cController {
    type Error = Error;

    fn write_read(
        &mut self,
        _address: u8,
        bytes_to_write: &[u8],
        bytes_to_read: &mut [u8],
    ) -> Result<(), Error> {
        self.events_to_device.send(Ok(I2cEvent::StartWrite))?;
        for byte in bytes_to_write {
            self.events_to_device
                .send(Ok(I2cEvent::ByteReceived(*byte)))?;
        }
        if !bytes_to_read.is_empty() {
            // StartRead should cause the first byte to sent.
            self.events_to_device.send(Ok(I2cEvent::StartRead))?;
            bytes_to_read[0] = self.bytes_from_device.recv()?;
            for out in &mut bytes_to_read[1..] {
                // For each subsequent byte, we need to request it when we're
                // ready.
                self.events_to_device.send(Ok(I2cEvent::NeedByte))?;
                *out = self.bytes_from_device.recv()?;
            }
        }
        // Our protocol handler doesn't wait for STOP, so the test may have
        // already finished and disconnected the host side. If the host side has
        // finished, then sending STOP will fail, which can be safely ignored.
        let _ = self.events_to_device.send(Ok(I2cEvent::Stop));
        Ok(())
    }
}

impl Write for FakeI2cController {
    type Error = Error;

    fn write(&mut self, address: u8, bytes_to_write: &[u8]) -> Result<(), Error> {
        self.write_read(address, bytes_to_write, &mut [])
    }
}

impl Read for FakeI2cController {
    type Error = Error;

    fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Error> {
        self.write_read(address, &[], buffer)
    }
}

impl I2cPeripheral for FakeI2cPeripheral {
    fn next_event(&mut self) -> nb::Result<I2cEvent, I2cError> {
        self.events_from_host
            .as_mut()
            .unwrap()
            .recv()
            .unwrap_or(Err(nb::Error::WouldBlock))
    }

    fn write_byte(&mut self, byte: u8) {
        let _ = self.bytes_to_host.as_mut().unwrap().send(byte);
    }

    fn reset(&mut self) {}
}

impl From<mpsc::RecvError> for Error {
    fn from(_: mpsc::RecvError) -> Self {
        Error::Disconnected
    }
}

impl<T> From<mpsc::SendError<T>> for Error {
    fn from(_: mpsc::SendError<T>) -> Self {
        Error::Disconnected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write() {
        let mut peripheral = FakeI2cPeripheral::new(|mut device| device.write(1, &[7, 8]).unwrap());
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::StartWrite));
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::ByteReceived(7)));
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::ByteReceived(8)));
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::Stop));
        assert_eq!(peripheral.next_event(), Err(nb::Error::WouldBlock));
    }

    #[test]
    fn test_write_read() {
        let mut peripheral = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            device.write_read(1, &[10], &mut out).unwrap();
            assert_eq!(out, [14, 15]);
        });
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::StartWrite));
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::ByteReceived(10)));
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::StartRead));
        peripheral.write_byte(14);
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::NeedByte));
        peripheral.write_byte(15);
        assert_eq!(peripheral.next_event(), Ok(I2cEvent::Stop));
        assert_eq!(peripheral.next_event(), Err(nb::Error::WouldBlock));
    }

    #[test]
    fn test_no_device_response() {
        // Our host code tries to write and read, but the device side does
        // nothing. The host side should get an error when device side gets
        // dropped.
        let peripheral = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            assert!(device.write_read(1, &[10], &mut out).is_err());
        });
        // On this run, we give the controller a chance to run first before we
        // drop the peripheral.
        std::thread::sleep(std::time::Duration::from_millis(1));
        core::mem::drop(peripheral);

        FakeI2cPeripheral::new(|mut device| {
            // On this run, we give the peripheral a bit of time to drop before
            // we try to talk to it.
            std::thread::sleep(std::time::Duration::from_millis(1));
            let mut out = [0u8; 2];
            assert!(device.write_read(1, &[10], &mut out).is_err());
        });
    }
}
