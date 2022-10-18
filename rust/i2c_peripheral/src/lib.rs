// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

pub trait I2cPeripheral {
    /// Returns the next event that needs to be handled.
    fn next_event(&mut self) -> nb::Result<I2cEvent, I2cError>;

    /// Write a byte to I2C. Should only be called when `I2cEvent::NeedByte` is
    /// received.
    fn write_byte(&mut self, byte: u8);

    fn reset(&mut self);
}

#[derive(Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum I2cEvent {
    /// Emitted when the I2C controller addresses us in read mode. First byte
    /// should be sent by calling `write_byte`.
    StartRead,

    /// Emitted when the I2C controller addresses us in write mode.
    StartWrite,

    /// Emitted when the I2C controller sends a stop signal.
    Stop,

    /// Emitted when the next byte is required. Call `write_byte`.
    NeedByte,

    /// Emitted when we receive a byte from the I2C controller.
    ByteReceived(u8),
}

#[derive(Eq, PartialEq, Debug)]
pub enum I2cError {
    Overrun,
    Underrun,
    BusError,
}

impl I2cError {
    pub fn as_str(&self) -> &'static str {
        match self {
            I2cError::Overrun => "Overrun",
            I2cError::Underrun => "Underrun",
            I2cError::BusError => "Bus error",
        }
    }
}
