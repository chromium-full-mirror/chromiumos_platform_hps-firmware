// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

use sodalite::HASH_LEN;
pub use sodalite::SIGN_LEN;
pub use sodalite::SIGN_SECRET_KEY_LEN;

pub const BLOCK_SIZE: usize = 256;

#[repr(C, packed(4))]
pub struct ImageHeader {
    pub magic: u32,
    pub version: u16,
    pub vector_addr: &'static u32,
    pub git_hash_ptr: &'static u8,
    pub git_hash_length: usize,
    pub sig: Signature,
}

#[repr(C)]
pub struct Signature {
    raw_bytes: [u8; SIGN_LEN],
}

impl Signature {
    pub const fn empty() -> Signature {
        Signature {
            raw_bytes: [0; SIGN_LEN],
        }
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.raw_bytes.copy_from_slice(&other.raw_bytes)
    }

    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

pub struct PrivateKey {
    raw_bytes: [u8; SIGN_SECRET_KEY_LEN],
}

impl PrivateKey {
    pub const fn from_bytes(raw_bytes: [u8; SIGN_SECRET_KEY_LEN]) -> PrivateKey {
        PrivateKey { raw_bytes }
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
    pub fn raw_bytes(&self) -> [u8; HASH_LEN] {
        self.0
    }
}

pub struct Hasher {
    hash: Hash,
}

impl Hasher {
    pub fn new() -> Hasher {
        Hasher {
            hash: Hash([0u8; 64]),
        }
    }

    pub fn add_block(&mut self, data: &[u8]) {
        let mut hash = [0u8; HASH_LEN];
        sodalite::hash(&mut hash, data);
        let mut hashes = [0u8; HASH_LEN * 2];
        hashes[..HASH_LEN].clone_from_slice(&self.hash.0);
        hashes[HASH_LEN..].clone_from_slice(&hash);
        sodalite::hash(&mut self.hash.0, &hashes);
    }

    pub fn finalize(self) -> Hash {
        self.hash
    }
}

pub fn sign_hash(hash: &Hash, private_key: &PrivateKey) -> Signature {
    let mut out = [0u8; SIGN_LEN + HASH_LEN];
    sodalite::sign_attached(&mut out, &hash.0, &private_key.raw_bytes);
    let mut signature = Signature {
        raw_bytes: [0u8; SIGN_LEN],
    };
    signature.raw_bytes.clone_from_slice(&out[..SIGN_LEN]);
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_signing() {
        let private_key = super::PrivateKey::from_bytes([42u8; crate::SIGN_SECRET_KEY_LEN]);

        let data = [1, 2, 3];

        let mut hasher = Hasher::new();
        for block in data.chunks(BLOCK_SIZE) {
            hasher.add_block(block);
        }
        let hash = hasher.finalize();

        let signature = sign_hash(&hash, private_key);
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
