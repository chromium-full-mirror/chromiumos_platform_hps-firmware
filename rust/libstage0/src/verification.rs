// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Code for verifying the stage1 slot.
use crate::WriteProtectState;
use mcu_common::Error;
use mcu_common::Hash;
use mcu_common::ImageHeader;
use mcu_common::SIGNATURE_LENGTH;
use mcu_common::STAGE1_SLOT_LENGTH;

const PAGE_SIZE: usize = 2 * 1024;
pub const PUBLIC_KEY_LENGTH: usize = ed25519_compact::PublicKey::BYTES;

/// A struct that indicates that stage1 is considered valid - either it has
/// passed a signature check or firmware write protect is deasserted.
pub struct ValidStage1 {
    _private: (),
}

fn stage1_header(slot: &[u8]) -> &'static ImageHeader {
    assert!(slot.len() >= core::mem::size_of::<ImageHeader>());
    assert!((slot.as_ptr() as usize % core::mem::align_of::<ImageHeader>()) == 0);
    unsafe { &*(slot.as_ptr() as *const ImageHeader) }
}

fn get_stage1_signature(slot: &[u8]) -> [u8; SIGNATURE_LENGTH] {
    stage1_header(slot).sig.unwrap()
}

/// Checks whether it's OK to load the application, returning an error if not.
pub fn detect_and_validate_stage1(
    slot: &[u8],
    public_key: &[u8; PUBLIC_KEY_LENGTH],
    wp_state: WriteProtectState,
    minimum_epoch: u16,
) -> Result<ValidStage1, Error> {
    if slot.len() != STAGE1_SLOT_LENGTH
        || (slot.as_ptr() as usize % core::mem::align_of::<ImageHeader>()) != 0
    {
        return Err(Error::Internal);
    }
    detect_stage1(slot)?;

    // If write-protect is asserted, then check that signature is valid.
    if wp_state == WriteProtectState::Asserted {
        if stage1_header(slot).epoch < minimum_epoch {
            return Err(Error::Stage1TooOld);
        }
        validate_stage1(slot, public_key).map_err(|_| Error::Stage1InvalidSignature)?;
    }

    Ok(ValidStage1 { _private: () })
}

fn detect_stage1(slot: &[u8]) -> Result<(), Error> {
    let magic = stage1_header(slot).magic;
    if magic == mcu_common::STAGE1_MAGIC {
        return Ok(());
    }
    Err(Error::Stage1NotFound)
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

/// Returns the largest minimum epoch number written to OTP. We determine that
/// an OTP double-word is a minimum epoch if the value is <= 0xffff. Values >
/// 0xffff are either blank OTP double-words, or some other value that isn't a
/// minimum epoch. This gives us the option to use the OTP area for additional
/// purposes in future.
pub fn determine_minimum_epoch(otp_area: &[u64]) -> u16 {
    let mut version = 0;
    for &value in otp_area {
        if value <= 0xffff {
            version = u16::max(version, value as u16);
        }
    }
    version
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

        // Set version.
        memory[1] = 0x10;

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
        detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Asserted,
            0,
        )
        .unwrap();

        // If the minimum epoch is greater than the actual epoch, it should fail.
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Asserted,
            0x11,
        )
        .is_err());

        // Change a byte inside our original payload and confirm that it fails.
        memory_bytes[1000] = 10;
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Asserted,
            0
        )
        .is_err());

        // Change a byte outside our original payload and confirm that it fails.
        memory_bytes[2000] = 10;
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Asserted,
            0
        )
        .is_err());

        // With write-protect deasserted, it should pass.
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Deasserted,
            0
        )
        .is_ok());

        // However with the magic value absent, it should fail.
        memory_bytes[0] = 0;
        assert!(detect_and_validate_stage1(
            memory_bytes,
            &DEV_PUBLIC_KEY,
            WriteProtectState::Deasserted,
            0
        )
        .is_err());
    }

    #[test]
    fn test_detect_and_validate_stage1_invalid_slot_length() {
        assert!(
            detect_and_validate_stage1(&[], &DEV_PUBLIC_KEY, WriteProtectState::Asserted, 0)
                .is_err()
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

    #[test]
    fn test_determine_minimum_epoch() {
        assert_eq!(0, determine_minimum_epoch(&[]));
        assert_eq!(0, determine_minimum_epoch(&[0xffff_ffff_ffff_ffff]));
        assert_eq!(0x10, determine_minimum_epoch(&[0x10]));
        assert_eq!(0x20, determine_minimum_epoch(&[0x10, 0x20]));
        assert_eq!(0x30, determine_minimum_epoch(&[0x30, 0x20]));
        assert_eq!(
            0x31,
            determine_minimum_epoch(&[
                0xffff_ffff_ffff_ffff,
                0x10,
                0xffff_1234_5678,
                0x31,
                0x12,
                0xffff_1234,
                0xffff_ffff_ffff_ffff,
            ])
        );
    }
}
