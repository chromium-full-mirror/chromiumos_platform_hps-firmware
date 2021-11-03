// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use mcu_common::Hash;
use mcu_common::HostImageHeader;
use mcu_common::Signature;
use mcu_common::SIGNATURE_LENGTH;
use mcu_common::{FLASH_PAGE_SZ, FLASH_WRITE_SZ};

/// Returns a mutable reference to the signature as well as the payload (the
/// data that the signature applies to).
pub fn split_signature_and_payload(file_bytes: &mut [u8]) -> Result<(&mut Signature, &[u8])> {
    if file_bytes.len() < std::mem::size_of::<HostImageHeader>() {
        bail!("Input file is too short to contain ImageHeader");
    }
    // Note, this is possibly technically invalid, since ImageHeder contains
    // references that won't be valid in the current memory space. We don't use
    // those references, but technically Rust would be permitted to dereference
    // these references. We also don't check that file_bytes is properly aligned
    // for an ImageHader, but again we don't use any fields that have alignment
    // more than 1.
    let header = unsafe { &mut *(&mut file_bytes[0] as *mut u8 as *mut HostImageHeader) };
    Ok((&mut header.sig, &file_bytes[..]))
}

/// Compute the digest of the payload image with the required length
/// extention for later signing.
pub fn hash_image_bytes(payload: &[u8], region_length: usize) -> Result<[u8; 32]> {
    let v = extend_image(payload, region_length);

    let mut hasher = Hash::new();
    for block in v.chunks(FLASH_PAGE_SZ) {
        hasher.update(block);
    }
    let hash = hasher.finalize();
    println!("signing with digest = {:#02X?}", hash);
    Ok(hash)
}

/// Return a extended image with a provided length.
fn extend_image(image: &[u8], extend_length: usize) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(image);

    // Determine the residue of remaining Bytes that
    // will complete the write size and pad with zeros.
    let rem = v.len() % FLASH_WRITE_SZ;
    v.resize(v.len() + rem, 0x00);

    // Further extend the image to fill the entire region length
    // provided by padding the remaining pages with 0xFF.
    v.resize(extend_length, 0xFF);

    v
}

pub fn sign_hash(hash: &[u8], private_key: &ed25519_compact::SecretKey) -> Signature {
    let out = private_key.sign(hash, None); // TODO(quasisec): should not be None.
    let mut signature = Signature {
        raw_bytes: [0u8; SIGNATURE_LENGTH],
    };
    signature
        .raw_bytes
        .clone_from_slice(&out[..SIGNATURE_LENGTH]);
    signature
}

#[cfg(test)]
mod test {
    #[test]
    fn test_signing() {
        let private_key = ed25519_compact::SecretKey::new([42u8; 64]);

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
                140, 198, 68, 19, 74, 99, 28, 165, 111, 54, 208, 177, 31, 95, 92, 150, 227, 87,
                153, 171, 120, 137, 65, 27, 153, 80, 10, 21, 249, 22, 71, 216, 150, 80, 25, 16,
                181, 153, 222, 95, 252, 75, 18, 21, 240, 14, 35, 124, 41, 79, 42, 85, 190, 70, 68,
                66, 164, 175, 17, 120, 238, 232, 248, 7
            ]
        );
    }

    #[test]
    fn test_hash_image_bytes() {
        // Ensure data size isn't an exact multiple of the flash write size (8) so that
        // zero-padding is tested.
        const OFFSET: usize = 32 + 1;
        let image: [u8; super::FLASH_PAGE_SZ + OFFSET] = [0xCC; super::FLASH_PAGE_SZ + OFFSET];
        let ext_len: usize = 4096;

        let ext_img_digest = super::hash_image_bytes(&image, ext_len).unwrap();
        assert_eq!(ext_img_digest.len(), 32);
        // Confirm hash of sample data is invariant.
        assert_eq!(
            ext_img_digest,
            [
                0x75, 0x4F, 0xD9, 0x7C, 0xE5, 0x81, 0x7C, 0xF3, 0x5A, 0xA2, 0x65, 0x82, 0xB8, 0x4C,
                0x35, 0xB9, 0x37, 0x38, 0xBA, 0x58, 0xE1, 0x7, 0x7C, 0xDC, 0xF9, 0x94, 0x95, 0x9D,
                0x57, 0xDD, 0x77, 0xE6,
            ]
        );
    }

    #[test]
    fn test_extend_image() {
        // Ensure data size isn't an exact multiple of the flash write size (8) so that
        // zero-padding is tested.
        const OFFSET: usize = 32 + 1;
        let image: [u8; super::FLASH_PAGE_SZ + OFFSET] = [0xCC; super::FLASH_PAGE_SZ + OFFSET];
        let ext_len: usize = 4096;

        let ext_img = super::extend_image(&image, ext_len);
        assert_eq!(ext_img.len(), ext_len);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + (OFFSET - 1)], 0xCC);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + OFFSET], 0x00);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + (OFFSET + 1)], 0xFF);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + (OFFSET * 2)], 0xFF);
    }
}
