// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(feature = "std")]
pub mod build_utils;

pub mod commands;
pub mod memory_banks;
pub mod registers;

use bitflags::bitflags;
mod common_host_interface;
mod errors;
mod mcu_debug_commands;
mod mem_block;

pub use common_host_interface::CommonHostInterface;
pub use errors::Error;
pub use hmac_sha256::Hash;
pub use mcu_debug_commands::McuDebugCommand;
pub use mem_block::MemBlock;

/// The length of the stage1 slot. This constant is used by stage0, so shouldn't
/// be changed once stage0 is locked.
pub const STAGE1_SLOT_LENGTH: usize = 64 * 1024;

/// STM32G0x1, RM0444 (Reference manual)
/// p.76 - 3.3.8 FLASH Main memory programming sequences
/// It is only possible to program a double word (2 x 32-bit data).
pub const FLASH_WRITE_SZ: usize = 4 * 2;

/// STM32G0x1, RM0444 (Reference manual)
/// p.70 3.3.1 FLASH memory organization.
/// Each page consists of eight rows of 256 Bytes.
pub const FLASH_PAGE_SZ: usize = 4 * 256 * 2;

/// A value that all debug commands start with when sent from hps-mon to the
/// MCU. This allows the MCU to resynchronize if it gets out of sync. If it
/// receives a command block that doesn't start with this value, it can skip
/// forward until it finds this value. 254 is chosen since it's a pretty
/// unlikely value to appear in data.
pub const DEBUG_COMMAND_START: u8 = 254;

/// Number of bytes in a debug command block.
pub const DEBUG_BYTES_PER_COMMAND: usize = 4;

/// The i2c address of the HPS.
pub const HPS_ADDRESS: u8 = 0x30;

/// The start address in the SPI flash at which the SOC ROM starts.
pub const SOC_ROM_OFFSET: u32 = 2 * 1024 * 1024;

/// The address of the start of flash.
pub const FLASH_START: u32 = 0x08000000;

/// The full size of the SPI flash.
pub const SPI_FLASH_SIZE: u32 = 16 * 1024 * 1024;

/// The offset in the SPI flash at which we write a test pattern.
pub const SPI_TEST_DATA_OFFSET: u32 = 15 * 1024 * 1024;

/// The address at which the application is written. The stage1 header will be
/// at this address. This constant is used by stage0, so shouldn't be changed
/// once stage0 is locked.
pub const APPLICATION_START_ADDRESS: u32 = 0x08010000;

/// The offset of vector table within the application. This constant is used by
/// stage0, so shouldn't be changed once stage0 is locked.
pub const APPLICATION_VECTOR_TABLE_OFFSET: u32 = 0x100;

// The address of the application vector table.
pub const APPLICATION_VECTOR_TABLE_ADDRESS: u32 =
    APPLICATION_START_ADDRESS + APPLICATION_VECTOR_TABLE_OFFSET;

/// The offset of the start of the signature within stage1.
pub const SIGNATURE_OFFSET: usize = 20;

/// The length of the signature in bytes.
pub const SIGNATURE_LENGTH: usize = 64;

/// Value at the start of the stage1 header.
pub const STAGE1_MAGIC: u32 = 0xC03FEFE;

/// ImageHeader is the structure found upon the prefix of a stage 1 image.
/// N.B., that all fields must be either unsigned integers that are fixed-sized
/// otherwise 64 bit signing hosts binaries will be built different sized field
/// to the 32 bit target target binaries, resulting in miss-aligned stage 1 HDR's.
#[repr(C)]
pub struct ImageHeader {
    pub magic: u32,
    pub version: u16,
    pub _reserved0: u32,
    pub _reserved1: u32,
    pub _reserved2: u32,
    pub sig: Signature,
}

#[repr(C)]
pub struct Signature {
    pub raw_bytes: [u8; SIGNATURE_LENGTH],
}

impl Signature {
    pub const fn empty() -> Signature {
        Signature {
            raw_bytes: [0; SIGNATURE_LENGTH],
        }
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.raw_bytes.copy_from_slice(&other.raw_bytes)
    }

    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    pub fn unwrap(&self) -> [u8; SIGNATURE_LENGTH] {
        self.raw_bytes
    }
}

impl ImageHeader {
    pub const fn empty() -> Self {
        Self {
            magic: STAGE1_MAGIC,
            version: 1,
            _reserved0: 0,
            _reserved1: 0,
            _reserved2: 0,
            sig: Signature::empty(),
        }
    }
}

/// Returns a magic number that a functioning HPS will always return from
/// register 0. This is used to confirm that a particular device on the I2C bus
/// is actually a HPS.
pub const fn hps_magic_code() -> u16 {
    let code = b"HPS";
    (((((code[0] - b'A') as u16) << 5) | ((code[1] - b'A') as u16)) << 5)
        | ((code[2] - b'A') as u16)
        | (1 << 15)
}

bitflags! {
    pub struct Status: u16 {
        const OK = 1 << 0;
        const FAULT = 1 << 1;
        const AVERIFY = 1 << 2;
        const ANONVER = 1 << 3;
        const WPON = 1 << 4;
        const WPOFF = 1 << 5;
        const STAGE1 = 1 << 8;
        const APPLREADY = 1 << 9;
        const COMMAND_IN_PROGRESS = 1 << 10;
        const STAGE0_LOCKED = 1 << 11;
        const STAGE0_PERM_LOCKED = 1 << 12;
        const ONE_TIME_INIT = 1 << 13;
    }
}

bitflags! {
    pub struct OptionBytesConfigRequest: u16 {
        /// Whether to reload the option bytes when done. Will trigger a reset.
        const RELOAD = 1 << 0;

        /// Whether to configure the the MCU so that reset won't wait for the
        /// reset pin to be pulled low before actually resetting.
        const RESET_PIN = 1 << 1;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_signature_offset() {
        // Check that the signature is at the expected offset in the header. If
        // we add or remove fields in the header, we don't want this to change,
        // since hpsd knows this offset.
        let header = ImageHeader {
            magic: 0,
            version: 0,
            _reserved0: 0,
            _reserved1: 0,
            _reserved2: 0,
            sig: Signature {
                raw_bytes: [0u8; SIGNATURE_LENGTH],
            },
        };
        let signature_offset =
            &header.sig.raw_bytes as *const _ as usize - &header as *const _ as usize;
        assert_eq!(signature_offset, SIGNATURE_OFFSET);
    }

    #[test]
    fn test_vector_table_alignment() {
        // The MCU requires that the vector table is aligned to a multiple of
        // 0x100 bytes.
        assert_eq!(APPLICATION_VECTOR_TABLE_ADDRESS % 0x100, 0);
    }

    #[test]
    fn test_stage1_within_flash() {
        // Ensure that the "FLASH" section doesn't extend beyond the end of the
        // actual flash.
        const FLASH_END_ADDRESS: u32 = 0x08020000;
        assert!(APPLICATION_START_ADDRESS + STAGE1_SLOT_LENGTH as u32 <= FLASH_END_ADDRESS);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_header_before_stage1() {
        // Ensure that "HDR" doesn't extend into "FLASH".
        assert!(
            APPLICATION_START_ADDRESS + APPLICATION_VECTOR_TABLE_OFFSET
                <= APPLICATION_VECTOR_TABLE_ADDRESS
        );
    }
}
