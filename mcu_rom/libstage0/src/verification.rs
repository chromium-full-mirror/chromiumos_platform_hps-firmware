// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::WriteProtectState;
/// Code for verifying the stage1 slot.
use mcu_common::Hash;
use mcu_common::TargetImageHeader;
use mcu_common::SIGNATURE_LENGTH;

const PAGE_SIZE: usize = 2 * 1024;
pub const PUBLIC_KEY_LENGTH: usize = ed25519_compact::PublicKey::BYTES;

pub enum Stage1Error {
    Missing,
    InvalidSignature,
}

/// A struct that indicates that stage1 is considered valid - either it has
/// passed a signature check or firmware write protect is deasserted.
pub struct ValidStage1 {
    firmware_version: [u16; 2], // Contains first 4B of validated signature.
    pub vector_address: *const u32,
}

unsafe impl Send for ValidStage1 {}

impl ValidStage1 {
    pub fn firmware_version(&self) -> &[u16; 2] {
        &self.firmware_version
    }
}

fn stage1_header(slot: &[u8]) -> &'static TargetImageHeader {
    unsafe { &*(slot.as_ptr() as *const TargetImageHeader) }
}

fn get_stage1_signature(slot: &[u8]) -> [u8; SIGNATURE_LENGTH] {
    stage1_header(slot).sig.unwrap()
}

fn decode_version(hdr_sig: &[u8; SIGNATURE_LENGTH]) -> [u16; 2] {
    [
        u16::from_be_bytes([hdr_sig[0], hdr_sig[1]]),
        u16::from_be_bytes([hdr_sig[2], hdr_sig[3]]),
    ]
}

/// Checks whether it's OK to load the application, returning an error if not.
pub fn detect_and_validate_stage1(
    slot: &[u8],
    public_key: &[u8; PUBLIC_KEY_LENGTH],
    wp_state: WriteProtectState,
) -> Result<ValidStage1, Stage1Error> {
    detect_stage1(slot)?;

    let stage1_validation = validate_stage1(slot, public_key);

    match stage1_validation {
        Ok(_) => Ok(ValidStage1 {
            firmware_version: get_stage1_version(slot),
            vector_address: stage1_header(slot).vector_addr,
        }),
        Err(_) => match wp_state {
            WriteProtectState::Asserted => Err(Stage1Error::InvalidSignature),
            WriteProtectState::Deasserted => Ok(ValidStage1 {
                firmware_version: get_stage1_version(slot),
                vector_address: stage1_header(slot).vector_addr,
            }),
        },
    }
}

fn detect_stage1(slot: &[u8]) -> Result<(), Stage1Error> {
    let magic = stage1_header(slot).magic;
    if magic == 0xC03FEFE {
        return Ok(());
    }
    Err(Stage1Error::Missing)
}

fn stage1_digest(slot: &[u8]) -> [u8; 32] {
    let mut hasher = Hash::new();

    // We hash the first page separately because we need to zero-out the part
    // that contains the signature. That's what was in the file when the digest
    // was computed during the signing process.
    let mut first_page = [0u8; PAGE_SIZE];
    first_page.copy_from_slice(&slot[..PAGE_SIZE]);
    let sig_offset =
        &stage1_header(slot).sig as *const _ as usize - stage1_header(slot) as *const _ as usize;
    for idx in 0..64 {
        first_page[sig_offset + idx] = 0x00;
    }
    hasher.update(&first_page);

    for page in slot[PAGE_SIZE..].chunks_exact(PAGE_SIZE) {
        // Page flash by 2K steps.
        hasher.update(&page);
    }
    hasher.finalize()
}

fn validate_stage1(
    slot: &[u8],
    public_key: &[u8; PUBLIC_KEY_LENGTH],
) -> Result<(), ed25519_compact::Error> {
    let stage1_digest = stage1_digest(slot);

    let hdr_sig = get_stage1_signature(slot);
    let sign = ed25519_compact::Signature::new(hdr_sig);
    let vkey = ed25519_compact::PublicKey::new(*public_key);

    vkey.verify(&stage1_digest, &sign)?;
    Ok(())
}

fn get_stage1_version(slot: &[u8]) -> [u16; 2] {
    let hdr_sig = get_stage1_signature(slot);
    decode_version(&hdr_sig)
}
