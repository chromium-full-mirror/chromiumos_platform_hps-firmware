// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Code for verifying the stage1 slot.
use crate::WriteProtectState;
use mcu_common::Hash;
use mcu_common::ImageHeader;
use mcu_common::LEGACY_STAGE1_SLOT_LENGTH;
use mcu_common::SIGNATURE_LENGTH;
use mcu_common::STAGE1_SLOT_LENGTH;

const PAGE_SIZE: usize = 2 * 1024;
pub const PUBLIC_KEY_LENGTH: usize = ed25519_compact::PublicKey::BYTES;

#[derive(Debug)]
pub enum Stage1Error {
    Missing,
    InvalidSignature,
    InvalidArgument,
}

/// A struct that indicates that stage1 is considered valid - either it has
/// passed a signature check or firmware write protect is deasserted.
pub struct ValidStage1 {
    firmware_version: [u16; 2], // Contains first 4B of validated signature.
}

impl ValidStage1 {
    pub fn firmware_version(&self) -> &[u16; 2] {
        &self.firmware_version
    }
}

fn stage1_header(slot: &[u8]) -> &'static ImageHeader {
    assert!(slot.len() >= core::mem::size_of::<ImageHeader>());
    assert!((slot.as_ptr() as usize % core::mem::align_of::<ImageHeader>()) == 0);
    unsafe { &*(slot.as_ptr() as *const ImageHeader) }
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
    if slot.len() != STAGE1_SLOT_LENGTH
        || (slot.as_ptr() as usize % core::mem::align_of::<ImageHeader>()) != 0
    {
        return Err(Stage1Error::InvalidArgument);
    }
    detect_stage1(slot)?;

    // If write-protect is asserted, then check that signature is valid.
    if wp_state == WriteProtectState::Asserted {
        let mut result = validate_stage1(slot, public_key);
        if result.is_err() {
            result = validate_legacy_signed_stage1(slot, public_key);
        }
        result.map_err(|_| Stage1Error::InvalidSignature)?
    }

    Ok(ValidStage1 {
        firmware_version: get_stage1_version(slot),
    })
}

fn detect_stage1(slot: &[u8]) -> Result<(), Stage1Error> {
    let magic = stage1_header(slot).magic;
    if magic == mcu_common::STAGE1_MAGIC {
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

/// Verifies a stage1 signature that was signed using a 64KB slot length. We
/// first check that everything after that 64KB is empty, then check that the
/// signature for the 64KB passes.
fn validate_legacy_signed_stage1(
    slot: &[u8],
    public_key: &[u8; PUBLIC_KEY_LENGTH],
) -> Result<(), ed25519_compact::Error> {
    // Make sure everything after the legacy slot is empty. If it's not, fail
    // the signature check.
    for byte in &slot[LEGACY_STAGE1_SLOT_LENGTH..] {
        if *byte != 0xff {
            return Err(ed25519_compact::Error::InvalidSignature);
        }
    }
    // Now do a signature check just on the legacy slot.
    validate_stage1(&slot[..LEGACY_STAGE1_SLOT_LENGTH], public_key)
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

#[cfg(test)]
mod tests {
    use super::*;

    const DEV_PUBLIC_KEY: [u8; PUBLIC_KEY_LENGTH] = [
        0x01, 0xB5, 0x9C, 0x83, 0xD3, 0x8D, 0x40, 0x9B, 0x71, 0x18, 0xA8, 0x38, 0x19, 0xEB, 0x4C,
        0x77, 0x22, 0x01, 0x9D, 0x3C, 0xB7, 0x27, 0x5A, 0x23, 0xC8, 0x45, 0x0C, 0x88, 0xB3, 0xB0,
        0x02, 0x6A,
    ];

    #[test]
    fn test_detect_and_validate_stage1_nomiri() {
        // Start with memory initially blank (0xff). We use u32 because the
        // header needs to be 4-byte aligned.
        let mut memory = vec![0xffff_ffff_u32; mcu_common::STAGE1_SLOT_LENGTH / 4];

        // Insert magic value into header.
        memory[0] = mcu_common::STAGE1_MAGIC;

        let memory_bytes = unsafe {
            core::slice::from_raw_parts_mut(memory.as_mut_ptr() as *mut u8, memory.len() * 4)
        };

        // This is the part of memory that we sign. Everything outside of this
        // memory range should be blank.
        let payload_bytes = &mut memory_bytes[..1600];

        // Insert header. This is required by hash_image_bytes.
        let header = ImageHeader::empty();
        const HEADER_LEN: usize = core::mem::size_of::<ImageHeader>();
        payload_bytes[..HEADER_LEN].copy_from_slice(unsafe {
            core::slice::from_raw_parts(&header as *const ImageHeader as *const u8, HEADER_LEN)
        });

        // Write some arbitrary data somewhere, so that it's not all blank.
        for i in 0..255_u8 {
            payload_bytes[1024 + i as usize] = i;
        }

        // Sign our "file".
        sign_rom::zero_signature(payload_bytes).unwrap();
        let hash = sign_rom::hash_image_bytes(payload_bytes).unwrap();
        let signature = sign_rom::sign_hash(&hash, &sign_rom::development_only_secret_key());
        sign_rom::write_signature(payload_bytes, &signature).unwrap();

        dbg!(&memory_bytes[..300]);
        dbg!(memory_bytes.len());

        // Check that this passes.
        detect_and_validate_stage1(memory_bytes, &DEV_PUBLIC_KEY, WriteProtectState::Asserted)
            .unwrap();

        // Change a byte inside our original payload and confirm that it fails.
        memory_bytes[1000] = 10;
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Asserted
        )
        .is_err());

        // Change a byte outside our original payload and confirm that it fails.
        memory_bytes[2000] = 10;
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Asserted
        )
        .is_err());

        // With write-protect deasserted, it should pass.
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Deasserted
        )
        .is_ok());

        // However with the magic value absent, it should fail.
        memory_bytes[0] = 0;
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Deasserted
        )
        .is_err());
    }

    #[test]
    fn test_detect_and_validate_stage1_invalid_slot_length() {
        assert!(
            detect_and_validate_stage1(&[], &DEV_PUBLIC_KEY, WriteProtectState::Asserted).is_err()
        );
    }

    #[test]
    fn test_stage1_header() {
        let mut memory = vec![0xffff_ffff_u32; 1024];
        memory[0] = mcu_common::STAGE1_MAGIC;

        let memory_bytes =
            unsafe { core::slice::from_raw_parts(memory.as_ptr() as *mut u8, memory.len() * 4) };

        assert_eq!(stage1_header(memory_bytes).magic, mcu_common::STAGE1_MAGIC);
    }
}
