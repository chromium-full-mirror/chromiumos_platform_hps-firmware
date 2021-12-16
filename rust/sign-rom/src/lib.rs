// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use mcu_common::Hash;
use mcu_common::Signature;
use mcu_common::FLASH_PAGE_SZ;
use mcu_common::SIGNATURE_LENGTH;
use mcu_common::SIGNATURE_OFFSET;

/// Writes `signature` into `file_bytes`.
pub fn write_signature(file_bytes: &mut [u8], signature: &Signature) -> Result<()> {
    const SIGNATURE_END: usize = SIGNATURE_OFFSET + SIGNATURE_LENGTH;
    if file_bytes.len() < SIGNATURE_END {
        bail!("Input file is too short to contain a signature");
    }
    file_bytes[SIGNATURE_OFFSET..SIGNATURE_END].copy_from_slice(signature.raw_bytes());
    Ok(())
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

    // Extend the image to fill the entire region length
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
    use mcu_common::Signature;

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
        // padding is tested.
        const OFFSET: usize = 32 + 1;
        let image: [u8; super::FLASH_PAGE_SZ + OFFSET] = [0xCC; super::FLASH_PAGE_SZ + OFFSET];
        let ext_len: usize = 4096;

        let ext_img_digest = super::hash_image_bytes(&image, ext_len).unwrap();
        assert_eq!(ext_img_digest.len(), 32);
        // Confirm hash of sample data is invariant.
        assert_eq!(
            ext_img_digest,
            [
                0xB1, 0x9D, 0xFA, 0xD3, 0x83, 0xB7, 0x57, 0xC3, 0xB7, 0x54, 0xEE, 0x78, 0x50, 0x35,
                0x23, 0xED, 0x67, 0x55, 0x6E, 0xEB, 0xE5, 0x54, 0x30, 0x12, 0xFA, 0x4A, 0xDE, 0xD5,
                0x1, 0x88, 0x96, 0xCE,
            ]
        );
    }

    #[test]
    fn test_extend_image() {
        // Ensure data size isn't an exact multiple of the flash write size (8) so that
        // padding is tested.
        const OFFSET: usize = 32 + 1;
        let image: [u8; super::FLASH_PAGE_SZ + OFFSET] = [0xCC; super::FLASH_PAGE_SZ + OFFSET];
        let ext_len: usize = 4096;

        let ext_img = super::extend_image(&image, ext_len);
        assert_eq!(ext_img.len(), ext_len);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + (OFFSET - 1)], 0xCC);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + OFFSET], 0xFF);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + (OFFSET + 1)], 0xFF);
        assert_eq!(ext_img[super::FLASH_PAGE_SZ + (OFFSET * 2)], 0xFF);
    }

    #[test]
    fn test_write_signature() {
        let mut buffer = [0; 100];
        let signature = Signature {
            raw_bytes: [42u8; 64],
        };
        super::write_signature(&mut buffer, &signature).unwrap();
        assert_eq!(buffer[mcu_common::SIGNATURE_OFFSET], 42);
    }

    #[test]
    fn test_signature_mut_too_short() {
        let mut buffer = [0; 70];
        let signature = Signature {
            raw_bytes: [42u8; 64],
        };
        assert!(super::write_signature(&mut buffer, &signature).is_err());
    }
}
