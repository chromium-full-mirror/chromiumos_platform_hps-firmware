// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use bitflags::bitflags;
use core::convert::TryFrom;

bitflags! {
    pub struct Error: u16 {
        /// An I2C underrun occurred.
        const HOST_I2C_UNDERRUN = 1 << 0;

        /// An error occurred writing to MCU flash.
        const MCU_FLASH_WRITE_ERROR = 1 << 1;

        /// A panic occurred.
        const PANIC = 1 << 2;

        /// An I2C bus error occurred.
        const HOST_I2C_BUS_ERROR = 1 << 3;

        /// An I2C overrun occurred.
        const HOST_I2C_OVERRUN = 1 << 4;

        /// Something is wrong with the camera.
        const CAMERA = 1 << 5;

        /// Something is wrong with the SPI flash.
        const SPI_FLASH = 1 << 6;

        /// Something was wrong with a request from the host.
        const HOST_I2C_BAD_REQUEST = 1 << 7;

        /// An attempt was made to use a buffer, but the buffer was not available,
        /// possibly because it was already in use.
        const BUFFER_NOT_AVAILABLE = 1 << 8;

        /// An attempt was made to write beyond the end of a buffer.
        const BUFFER_OVERRUN = 1 << 9;

        /// The SPI flash does not match the embedded hash, an update is required.
        const SPI_FLASH_NOT_VERIFIED = 1 << 10;

        /// TFLITE Micro reported an error.
        const TFLITE_FAILURE = 1 << 11;

        /// A self test that was requested by the host has failed.
        const SELF_TEST_FAILED = 1 << 12;

        /// Communication between the FPGA and the MCU encountered an error.
        const FPGA_MCU_COMM_ERROR = 1 << 13;
    }
}

impl TryFrom<u16> for Error {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Error::from_bits(value).ok_or(())
    }
}

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
        assert_eq!(Error::HOST_I2C_BUS_ERROR.bits(), 8u16);
    }
}
