// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Enable the feature `dev` to get features that are only useful for
//! development and testing purposes.

use crate::registers;
use crate::status;
use crate::Error;
use i2c_peripheral::I2cError;

pub struct CommonHostInterface {
    error_bits: u16,
}

impl CommonHostInterface {
    pub fn new() -> Self {
        Self { error_bits: 0 }
    }

    pub fn read_register(&self, register: u8) -> Option<u16> {
        Some(match register {
            registers::MAGIC => crate::hps_magic_code(),
            registers::ERRORS => self.error_bits,
            registers::SYSTEM_STATUS if self.error_bits != 0 => status::FAULT,
            registers::SYSTEM_STATUS => status::OK,
            _ => return self.read_dev_register(register),
        })
    }

    pub fn report_error(&mut self, error: Error) {
        self.error_bits |= u16::from(error);
    }

    /// Report an error that occurred while communicating with the host via I2C.
    pub fn report_i2c_host_error(&mut self, error: I2cError) {
        self.report_error(match error {
            I2cError::Overrun => Error::HostI2cOverrun,
            I2cError::Underrun => Error::HostI2cUnderrun,
            I2cError::BusError => Error::HostI2cBusError,
        });
    }

    #[cfg(feature = "dev")]
    fn read_dev_register(&self, register: u8) -> Option<u16> {
        Some(match register {
            registers::dev::SLOW_READ => dev::fib(self.error_bits.count_ones() as u16 + 20),
            _ => return None,
        })
    }

    #[cfg(not(feature = "dev"))]
    fn read_dev_register(&self, _: u8) -> Option<u16> {
        None
    }
}

#[cfg(feature = "dev")]
mod dev {
    /// Computes a fibonacci number. This is just intended to be a somewhat slow
    /// operation that the compiler hopefully won't optimize away.
    pub(crate) fn fib(n: u16) -> u16 {
        if n <= 1 {
            0
        } else {
            fib(n - 1) + fib(n - 2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_reporting() {
        let mut interface = CommonHostInterface::new();
        assert_eq!(interface.read_register(registers::ERRORS), Some(0));
        assert_eq!(
            interface.read_register(registers::SYSTEM_STATUS),
            Some(status::OK)
        );
        interface.report_i2c_host_error(I2cError::Overrun);
        assert_eq!(
            interface.read_register(registers::ERRORS),
            Some(Error::HostI2cOverrun.unwrap())
        );
        interface.report_i2c_host_error(I2cError::Underrun);
        assert_eq!(
            interface.read_register(registers::ERRORS),
            Some(Error::HostI2cOverrun.unwrap() | Error::HostI2cUnderrun.unwrap())
        );
    }

    #[test]
    fn test_magic() {
        assert_eq!(
            CommonHostInterface::new().read_register(registers::MAGIC),
            Some(40434)
        );
    }
}
