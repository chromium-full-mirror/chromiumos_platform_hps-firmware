// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Enable the feature `dev` to get features that are only useful for
//! development and testing purposes.

use crate::registers;
use crate::Error;
use crate::Status;

pub struct CommonHostInterface {
    error: Error,
}

impl CommonHostInterface {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_register(&self, register: u8) -> Option<u16> {
        Some(match register {
            registers::MAGIC => crate::hps_magic_code(),
            registers::ERROR => self.error.into(),
            registers::SYSTEM_STATUS if self.error != Error::None => Status::FAULT.bits(),
            registers::SYSTEM_STATUS => Status::OK.bits(),
            _ => return self.read_dev_register(register),
        })
    }

    pub fn report_error(&mut self, error: Error) {
        // We only keep track of the first error reported. This is quite likely
        // to be the cause of any subsequent errors, so is the most important
        // one to track.
        if self.error == Error::None {
            self.error = error;
        }
    }

    #[cfg(feature = "dev")]
    fn read_dev_register(&self, register: u8) -> Option<u16> {
        Some(match register {
            registers::dev::SLOW_READ => dev::fib(u16::from(self.error).count_ones() as u16 + 20),
            _ => return None,
        })
    }

    #[cfg(not(feature = "dev"))]
    fn read_dev_register(&self, _: u8) -> Option<u16> {
        None
    }
}

impl Default for CommonHostInterface {
    fn default() -> Self {
        Self { error: Error::None }
    }
}

#[cfg(feature = "dev")]
mod dev {
    /// Computes a fibonacci number. This is just intended to be a somewhat slow
    /// operation that the compiler hopefully won't optimize away.
    pub(crate) fn fib(n: u16) -> u16 {
        match n {
            0 => 0,
            1 => 1,
            n => fib(n - 1) + fib(n - 2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_reporting() {
        let mut interface = CommonHostInterface::new();
        assert_eq!(interface.read_register(registers::ERROR), Some(0));
        assert_eq!(
            interface.read_register(registers::SYSTEM_STATUS),
            Some(Status::OK.bits())
        );
        interface.report_error(Error::HostI2cOverrun);
        assert_eq!(
            interface.read_register(registers::ERROR),
            Some(u16::from(Error::HostI2cOverrun))
        );
        // Reporting a second error has no effect, since only the first error
        // reported is known.
        interface.report_error(Error::HostI2cUnderrun);
        assert_eq!(
            interface.read_register(registers::ERROR),
            Some(u16::from(Error::HostI2cOverrun))
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
