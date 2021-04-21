// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

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
            _ => return None,
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
}
