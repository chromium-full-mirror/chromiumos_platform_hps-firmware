// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::From;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum Error {
    None = 0,

    /// An I2C underrun occurred.
    HostI2cUnderrun = 0x0001,

    /// An error occurred writing to MCU flash.
    McuFlashWriteError = 0x0002,

    /// A panic occurred.
    Panic = 0x0004,

    /// An I2C bus error occurred.
    HostI2cBusError = 0x0008,

    /// An I2C overrun occurred.
    HostI2cOverrun = 0x0010,

    /// Something is wrong with the camera.
    Camera = 0x0020,

    /// Something is wrong with the SPI flash.
    SpiFlash = 0x0040,

    /// Something was wrong with a request from the host.
    HostI2cBadRequest = 0x0080,

    /// An attempt was made to use a buffer, but the buffer was not available,
    /// possibly because it was already in use.
    BufferNotAvailable = 0x0100,

    /// An attempt was made to read or write beyond the end of a buffer.
    BufferOverrun = 0x0200,

    /// The SPI flash does not match the embedded hash, an update is required.
    SpiFlashNotVerified = 0x0400,

    /// TFLITE Micro reported an error.
    TfliteFailure = 0x0800,

    /// A self test that was requested by the host has failed.
    SelfTestFailed = 0x1000,

    /// Communication between the FPGA and the MCU encountered an error.
    FpgaMcuCommError = 0x2000,

    /// The FPGA application did not send an update within the expected time.
    FpgaTimeout = 0x4000,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The Debug implementation for bitfields is good enough for displaying
        // our errors.
        write!(f, "{:?}", self)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryFrom;

    #[test]
    fn test_from_int() {
        assert_eq!(Error::try_from(8), Ok(Error::HostI2cBusError));
    }

    #[test]
    fn test_to_int() {
        assert_eq!(u16::from(Error::HostI2cBusError), 8u16);
    }
}
