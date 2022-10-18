// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::num::Wrapping;

pub const CONFIGURATION_SIZE: usize = core::mem::size_of::<Configuration>();
pub const STATUS_SIZE: usize = core::mem::size_of::<Status>();
use core::str::FromStr;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Default)]
pub struct ApplicationState {
    pub configuration: Configuration,
    pub status: Status,
    /// The number of times the FPGA has started. This is not part of `status`
    /// because `status` is only ever written by the FPGA, never read and we
    /// want a persistent count.
    pub(crate) fpga_boot_count: Wrapping<u16>,
    pub fpga_rom_version: u8,
    pub camera_test_iterations: u16,
}

/// The current configuration of the HPS. This is writable by the host over I2C
/// and readable by the FPGA soft CPU over SPI. The FPGA reads the entire struct
/// as bytes, hence the repr(C). The host writes parts individually via
/// registers.
#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Configuration {
    pub enabled_features: u16,
    pub camera_config: u16,
}

/// Current status of the HPS. This is writable by the FPGA and is read by the
/// host.
#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Status {
    pub person_status: u16,
    /// A copy of `Configuration::enabled_features` as used by the last
    /// inference loop.
    pub enabled_features: u16,
    pub loop_count: u16,
    pub second_person_status: u16,
}

#[cfg(feature = "image-transfer")]
impl ApplicationState {
    pub fn i2c_image_transfer_enabled(&self) -> bool {
        self.configuration.enabled_features & 0x80 != 0
    }
}
impl Status {
    /// Returns `bytes` as a `Status`. All our CPUs use little endian. This
    /// function is not portable to big endian.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= STATUS_SIZE);
        // safety: we're reading an initialized byte array at least as large as
        // the size of a Status. There are no bit patterns that would give an
        // invalid Status.
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Status) }
    }

    /// Returns `self` as bytes. All our CPUs use little endian. This function
    /// is not portable to big endian.
    pub fn to_bytes(&self) -> [u8; STATUS_SIZE] {
        // safety: We're reading `self` as bytes. Any value of `self` will give
        // a valid byte array.
        unsafe { core::ptr::read_unaligned(self as *const Status as *const [u8; STATUS_SIZE]) }
    }
}

impl Configuration {
    /// Returns `bytes` as a `Configuration`. All our CPUs use little endian.
    /// This function is not portable to big endian.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= CONFIGURATION_SIZE);
        // safety: we're reading an initialized byte array at least as large as
        // the size of a Status. There are no bit patterns that would give an
        // invalid Status.
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Configuration) }
    }

    /// Returns `self` as bytes. All our CPUs use little endian. This function
    /// is not portable to big endian.
    pub fn to_bytes(&self) -> [u8; CONFIGURATION_SIZE] {
        // safety: We're reading `self` as bytes. Any value of `self` will give
        // a valid byte array.
        unsafe {
            core::ptr::read_unaligned(
                self as *const Configuration as *const [u8; CONFIGURATION_SIZE],
            )
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum Rotation {
    /// Camera is upright. No rotation is required.
    None = 0,
    /// Camera is mounted sidewase. Image needs to be rotation 90 degrees
    /// clockwise.
    Clockwise = 1,
}

impl FromStr for Rotation {
    type Err = &'static str;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        match s {
            "none" => Ok(Rotation::None),
            "clockwise" => Ok(Rotation::Clockwise),
            _ => Err("Invalid rotation (supported values are 'none' and 'clockwise'"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configuration_byte_conversion() {
        let configuration = Configuration {
            enabled_features: 0x42,
            camera_config: Rotation::Clockwise.into(),
        };
        let configuration2 = Configuration::from_bytes(&configuration.to_bytes());
        assert_eq!(configuration, configuration2);
    }

    #[test]
    fn test_status_byte_conversion() {
        let status = Status {
            person_status: 0x1234,
            enabled_features: 0xabcd,
            loop_count: 0x5678,
            second_person_status: 0x9,
        };
        let status2 = Status::from_bytes(&status.to_bytes());
        assert_eq!(status, status2);
    }
}
