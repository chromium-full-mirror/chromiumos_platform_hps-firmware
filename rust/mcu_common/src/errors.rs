// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::From;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
// TODO: Fix naming of enum variants.
#[allow(non_camel_case_types)]
pub enum Error {
    None = 0,

    /// An I2C underrun occurred.
    HOST_I2C_UNDERRUN = 0x0001,

    /// An error occurred writing to MCU flash.
    MCU_FLASH_WRITE_ERROR = 0x0002,

    /// A panic occurred.
    PANIC = 0x0004,

    /// An I2C bus error occurred.
    HOST_I2C_BUS_ERROR = 0x0008,

    /// An I2C overrun occurred.
    HOST_I2C_OVERRUN = 0x0010,

    /// Something is wrong with the camera.
    CAMERA = 0x0020,

    /// Something is wrong with the SPI flash.
    SPI_FLASH = 0x0040,

    /// Something was wrong with a request from the host.
    HOST_I2C_BAD_REQUEST = 0x0080,

    /// An attempt was made to use a buffer, but the buffer was not available,
    /// possibly because it was already in use.
    BUFFER_NOT_AVAILABLE = 0x0100,

    /// An attempt was made to read or write beyond the end of a buffer.
    BUFFER_OVERRUN = 0x0200,

    /// The SPI flash does not match the embedded hash, an update is required.
    SPI_FLASH_NOT_VERIFIED = 0x0400,

    /// TFLITE Micro reported an error.
    TFLITE_FAILURE = 0x0800,

    /// A self test that was requested by the host has failed.
    SELF_TEST_FAILED = 0x1000,

    /// Communication between the FPGA and the MCU encountered an error.
    FPGA_MCU_COMM_ERROR = 0x2000,

    /// The FPGA application did not send an update within the expected time.
    FPGA_TIMEOUT = 0x4000,
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
        assert_eq!(Error::try_from(8), Ok(Error::HOST_I2C_BUS_ERROR));
    }

    #[test]
    fn test_to_int() {
        assert_eq!(u16::from(Error::HOST_I2C_BUS_ERROR), 8u16);
    }
}
