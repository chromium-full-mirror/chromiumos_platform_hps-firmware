// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::From;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum Error {
    /// An I2C underrun occurred.
    HostI2cUnderrun = 1 << 0,

    /// An error occurred writing to MCU flash.
    McuFlashWriteError = 1 << 1,

    /// A panic occurred.
    Panic = 1 << 2,

    /// An I2C bus error occurred.
    HostI2cBusError = 1 << 3,

    /// An I2C overrun occurred.
    HostI2cOverrun = 1 << 4,
}

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
