// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

pub mod commands;
pub mod memory_banks;
pub mod registers;

mod common_host_interface;
mod errors;
mod mcu_debug_commands;
mod mem_block;

pub use common_host_interface::CommonHostInterface;
pub use errors::Error;
pub use hmac_sha256::Hash;
pub use mcu_debug_commands::McuDebugCommand;
pub use mem_block::MemBlock;

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

/// ImageHeader is the structure found upon the prefix of a stage 1 image.
/// N.B., that all fields must be either unsigned integers that are fixed-sized
/// otherwise 64 bit signing hosts binaries will be built different sized field
/// to the 32 bit target target binaries, resulting in miss-aligned stage 1 HDR's.
#[repr(C, packed(4))]
pub struct ImageHeader<T, R> {
    pub magic: u32,
    pub version: u16,
    pub vector_addr: T,       // host := u32, target := &'static u32,
    pub git_hash_ptr: R,      // host := u32, target := &'static u8,
    pub git_hash_length: u32, // host := u32, target := usize - resolved by cast.
    pub sig: Signature,
}

// TargetImageHeader is a type-alias of ImageHeader defined for the purpose
// of being used by run-time executable paths in MCU firmware. Reference
// fields are defined to be fixed-width u32 primitive types as the propositional
// starting point for their intended use-case on bare metal.
pub type TargetImageHeader = ImageHeader<&'static u32, &'static u8>;

// HostImageHeader is a type-alias of ImageHeader defined for the purpose
// of being used by either host-side compile-time or host-side run-time executable
// paths within host tooling. Reference fields are defined to be machine local types
// as the propositional starting point for their intended use-case with host tooling
// and later to be reinterpreted by metal.
pub type HostImageHeader = ImageHeader<u32, u32>;

#[repr(C)]
pub struct Signature {
    pub raw_bytes: [u8; 64],
}

impl Signature {
    pub const fn empty() -> Signature {
        Signature { raw_bytes: [0; 64] }
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.raw_bytes.copy_from_slice(&other.raw_bytes)
    }

    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    pub fn unwrap(&self) -> [u8; 64] {
        self.raw_bytes
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

pub mod status {
    pub const OK: u16 = 1 << 0;
    pub const FAULT: u16 = 1 << 1;
    pub const AVERIFY: u16 = 1 << 2;
    pub const ANONVER: u16 = 1 << 3;
    pub const WPON: u16 = 1 << 4;
    pub const WPOFF: u16 = 1 << 5;
    pub const APPLRUN: u16 = 1 << 8;
    pub const APPLREADY: u16 = 1 << 9;
}
