// Copyright 2021 The ChromiumOS Authors
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

    /// A panic occurred on the previous boot.
    Panic = 0x0004,

    /// Code running on the FPGA panicked.
    FpgaPanic = 0x0005,

    /// Code running on the FPGA caused an exception.
    FpgaException = 0x0006,

    /// An I2C bus error occurred.
    HostI2cBusError = 0x0008,

    /// An I2C overrun occurred.
    HostI2cOverrun = 0x0010,

    /// Something went wrong with I2C communication to the camera.
    CameraI2c = 0x0020,

    /// Timeout receiving image data from the camera.
    CameraImageTimeout = 0x0021,

    /// The camera reported an unexpected ID.
    CameraUnexpectedId = 0x0022,

    /// The camera unexpectedly reset its configuration.
    CameraUnexpectedReset = 0x0023,

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

    /// Stage1 could not be found.
    Stage1NotFound = 0x4001,

    /// Stage1 does not satisfy epoch requirements.
    Stage1TooOld = 0x4002,

    /// Stage1 signature check failed.
    Stage1InvalidSignature = 0x4003,

    /// An internal constraint was violated.
    Internal = 0x4004,

    /// The MCU flash detected a non-recoverable ECC error.
    McuFlashEcc = 0x4005,

    /// A non-maskable interrupt was raised on the MCU for unknown reasons.
    McuNmi = 0x4006,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The Debug implementation for bitfields is good enough for displaying
        // our errors.
        write!(f, "{:?}", self)
    }
}

impl From<hm01b0::Error> for Error {
    fn from(error: hm01b0::Error) -> Self {
        match error {
            hm01b0::Error::I2cError => Self::CameraI2c,
            hm01b0::Error::InvalidValues => Self::Internal,
            hm01b0::Error::PartialReset => Self::CameraUnexpectedReset,
        }
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
