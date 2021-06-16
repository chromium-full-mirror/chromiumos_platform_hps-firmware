// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

use sodalite::HASH_LEN;
pub use sodalite::SIGN_SECRET_KEY_LEN;

pub mod commands;
pub mod memory_banks;
pub mod registers;

mod common_host_interface;
mod errors;
mod mcu_debug_commands;
mod mem_block;

pub use common_host_interface::CommonHostInterface;
pub use errors::Error;
pub use mcu_debug_commands::McuDebugCommand;
pub use mem_block::MemBlock;

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
    raw_bytes: [u8; 64],
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

pub struct PrivateKey {
    raw_bytes: [u8; 64],
}

impl PrivateKey {
    pub const fn from_bytes(raw_bytes: [u8; 64]) -> PrivateKey {
        PrivateKey { raw_bytes }
    }
    pub fn as_bytes(&self) -> [u8; 64] {
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

#[derive(Debug)]
pub struct Hash(pub [u8; HASH_LEN]);

impl Hash {
    pub fn new() -> Hash {
        Hash([0u8; 64])
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut hash = [0u8; HASH_LEN];
        sodalite::hash(&mut hash, data);
        let mut hashes = [0u8; HASH_LEN * 2];
        hashes[..HASH_LEN].clone_from_slice(&self.0);
        hashes[HASH_LEN..].clone_from_slice(&hash);
        sodalite::hash(&mut self.0, &hashes);
    }

    pub fn finalize(self) -> Hash {
        self
    }

    pub fn as_bytes(&self) -> [u8; HASH_LEN] {
        self.0
    }
}

pub fn sign_hash(hash: &Hash, private_key: &PrivateKey) -> Signature {
    let mut out = [0u8; 64 + HASH_LEN]; /* sign_len=64 */
    sodalite::sign_attached(&mut out, &hash.0, &private_key.raw_bytes);
    let mut signature = Signature {
        raw_bytes: [0u8; 64],
    };
    signature.raw_bytes.clone_from_slice(&out[..64]);
    signature
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_signing() {
        let private_key = super::PrivateKey::from_bytes([42u8; crate::SIGN_SECRET_KEY_LEN]);

        let data = [1, 2, 3];

        let mut hasher = super::Hash::new();
        for block in data.chunks(super::FLASH_PAGE_SZ) {
            hasher.update(block);
        }
        let hash = hasher.finalize();

        let signature = super::sign_hash(&hash, &private_key);
        assert_eq!(
            &signature.raw_bytes[..],
            &[
                3, 191, 2, 78, 224, 137, 169, 107, 177, 141, 152, 180, 238, 58, 93, 27, 179, 45,
                190, 220, 22, 54, 207, 40, 65, 229, 229, 217, 136, 28, 162, 154, 131, 152, 194, 71,
                9, 111, 239, 34, 57, 99, 85, 7, 5, 200, 194, 87, 49, 23, 209, 182, 187, 164, 128,
                191, 61, 229, 130, 1, 32, 102, 25, 15
            ]
        );
    }
}
