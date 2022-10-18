// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Enable the feature `dev` to get features that are only useful for
//! development and testing purposes.

use crate::registers::Register;
use crate::Error;
use crate::Status;
use core::cell::UnsafeCell;
use core::mem::size_of;

static ERROR: SyncUnsafeCell<Error> = SyncUnsafeCell::new(Error::None);

// TODO(dcallagh): replace with the real SyncUnsafeCell after
// https://github.com/rust-lang/rust/issues/95439
#[repr(transparent)]
struct SyncUnsafeCell<T>(UnsafeCell<T>);
impl<T> SyncUnsafeCell<T> {
    const fn new(value: T) -> Self {
        SyncUnsafeCell(UnsafeCell::new(value))
    }
}
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

// Compile-time assertion that Error will fit into a single load/store.
// This is an important assumption for the unsafe code below.
const _: () = {
    if size_of::<Error>() > 4 {
        panic!("The Error type must be small enough to load/store with a single instruction");
    }
};

/// Store an error in the global error state. This is intended to be called from the NMI handler
/// context, where there is no way to access normal RTIC-managed resources. Prefer to call
/// CommonHostInterface::report_error() instead.
pub fn report_global_error(error: Error) {
    // We only keep track of the first error reported. This is quite likely
    // to be the cause of any subsequent errors, so is the most important
    // one to track.
    //
    // Safety: This function is called from an exception handler, which can preempt anything else,
    // so there is a race window between reading the ERROR pointer and writing the new ERROR value.
    // However, because we only ever set a non-None error, and the presence of any non-None error
    // is considered fatal (FAULT bit will be reported), if there is a race we are guaranteed that
    // one or the other error value will end up being stored. This assumption is valid because
    // writing the ERROR value compiles down to a single store instruction.
    unsafe {
        if *ERROR.0.get() == Error::None {
            *ERROR.0.get() = error;
        }
    }
}

pub struct CommonHostInterface {
    error: Error,
}

// TODO(dcallagh): put #[default] on Error after Rust 1.62
impl Default for CommonHostInterface {
    fn default() -> Self {
        CommonHostInterface { error: Error::None }
    }
}

impl CommonHostInterface {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_register(&self, register: Register) -> Option<u16> {
        Some(match register {
            Register::Magic => crate::hps_magic_code(),
            Register::Error => self.get_error().into(),
            Register::SystemStatus => {
                if self.get_error() != Error::None {
                    Status::FAULT.bits()
                } else {
                    Status::OK.bits()
                }
            }
            _ => return self.read_dev_register(register),
        })
    }

    pub fn report_error(&mut self, error: Error) {
        if self.error == Error::None {
            self.error = error;
        }
    }

    pub fn get_error(&self) -> Error {
        // Safety: reading the ERROR value compiles to a single load instruction, so we
        // are guaranteed to always read a valid value even in the presence of preemption.
        let global_error = unsafe { *ERROR.0.get() };
        if global_error != Error::None {
            global_error
        } else {
            self.error
        }
    }

    #[cfg(feature = "dev")]
    fn read_dev_register(&self, register: Register) -> Option<u16> {
        Some(match register {
            Register::SlowRead => dev::fib(u16::from(self.get_error()).count_ones() as u16 + 20),
            _ => return None,
        })
    }

    #[cfg(not(feature = "dev"))]
    fn read_dev_register(&self, _: Register) -> Option<u16> {
        None
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
        assert_eq!(interface.read_register(Register::Error), Some(0));
        assert_eq!(
            interface.read_register(Register::SystemStatus),
            Some(Status::OK.bits())
        );
        interface.report_error(Error::HostI2cOverrun);
        assert_eq!(
            interface.read_register(Register::Error),
            Some(u16::from(Error::HostI2cOverrun))
        );
        // Reporting a second error has no effect, since only the first error
        // reported is known.
        interface.report_error(Error::HostI2cUnderrun);
        assert_eq!(
            interface.read_register(Register::Error),
            Some(u16::from(Error::HostI2cOverrun))
        );
    }

    #[test]
    fn test_magic() {
        assert_eq!(
            CommonHostInterface::new().read_register(Register::Magic),
            Some(40434)
        );
    }
}
