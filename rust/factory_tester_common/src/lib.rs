// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Code shared between the test controller and the code that runs on the HPS
//! MCU.

#![cfg_attr(not(test), no_std)]

use core::convert::Infallible;
use core::fmt::Debug;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

/// The I2C address that the MCU test program listens on to send data back to
/// the test controller.
pub const MCU_TESTER_I2C_ADDRESS: u8 = 0x20;

/// The address at which we load our MCU code. This needs to be after the memory
/// used by the factory bootloader. This address must be the same as what get
/// written to memory.x by build.rs for the factory_test_mcu crate.
pub const MCU_LOAD_ADDRESS: u32 = 0x20002700;

/// The ID of the MCU used in the HPS. STM32G07xxx/08xxx - see AN2606 rev 49,
/// table 152.
pub const EXPECTED_CHIP_ID: u16 = 0x460;

/// See AN2606 for the I2C address for each chip. 0x51 is STM32G07xxx/08xxx.
pub const FACTORY_BOOTLOADER_I2C_ADDRESS: u8 = 0x51;

/// Status code returned by the MCU to indicate that the test passed.
pub const TEST_STATUS_OK: u8 = 0;

/// Number of bytes in the test status payload.
pub const TEST_STATUS_SIZE: usize = core::mem::size_of::<TestStatus>();

/// Identifies a mode of failure when testing a HPS board. The numeric
/// identifiers are used both for communication between the test controller and
/// the MCU as well as for communicating the test status to a human - e.g. by
/// displaying a numeric error code on an LED display or by blinking and LED
/// some number of times.
#[derive(Debug, Copy, Clone, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Error {
    Internal = 1,
    SystemBootloader = 2,
    UnexpectedMcuId = 3,
    CameraI2c = 4,
    UnexpectedCameraId = 5,
    SpiFlashCommunication = 6,
    UnexpectedSpiFlashId = 7,
    FpgaNotStarted = 8,
    IncorrectImageData = 9,
    InterruptLine = 10,
    FwwpLine = 11,
    FirmwareAlreadyLoaded = 12,
    NoCameraData = 13,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TestStatus {
    pub error_code: u8,
    pub outputs: TestOutputs,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TestOutputs {
    pub spi_flash_id: SpiFlashId,
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        Error::Internal
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SpiFlashId {
    pub manufacturer_id: u8,
    pub device_id: [u8; 2],
}

pub const ALLOWED_SPI_FLASH_IDS: &[SpiFlashId] = &[
    // Gigadevices GD25LQ128D
    SpiFlashId {
        manufacturer_id: 0xC8,
        device_id: [0x60, 0x18],
    },
];

impl TestStatus {
    pub fn from_bytes(bytes: &[u8; TEST_STATUS_SIZE]) -> TestStatus {
        // safety: we're reading an initialized byte array with the same size as
        // a TestStatus. There are no bit patterns that would give an invalid
        // TestStatus.
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const TestStatus) }
    }

    pub fn to_bytes(&self) -> [u8; TEST_STATUS_SIZE] {
        // safety: We're reading `self` as bytes. Any value of `self` will give
        // a valid byte array.
        unsafe {
            core::ptr::read_unaligned(self as *const TestStatus as *const [u8; TEST_STATUS_SIZE])
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_status_byte_conversion() {
        let status = TestStatus {
            error_code: 4,
            outputs: TestOutputs {
                spi_flash_id: SpiFlashId {
                    manufacturer_id: 5,
                    device_id: [6, 7],
                },
            },
        };
        assert_eq!(TestStatus::from_bytes(&status.to_bytes()), status);
        assert_eq!(status.to_bytes(), [4, 5, 6, 7]);
    }
}
