// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use ed25519_compact::SecretKey;
use mcu_common::Hash;
use mcu_common::ImageHeader;
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

/// Returns the secret key used for development. This key must not be used for
/// production devices.
pub fn development_only_secret_key() -> SecretKey {
    ed25519_compact::SecretKey::new([
        0x9B, 0x25, 0x37, 0xA1, 0xDB, 0x16, 0x5E, 0xDC, 0x62, 0x23, 0x52, 0x8A, 0xD7, 0x5B, 0xE4,
        0x46, 0xF7, 0x79, 0xDC, 0x6B, 0x62, 0x8D, 0x60, 0xF0, 0x5D, 0x96, 0x95, 0x61, 0xAF, 0x50,
        0xCF, 0xD5, 0x01, 0xB5, 0x9C, 0x83, 0xD3, 0x8D, 0x40, 0x9B, 0x71, 0x18, 0xA8, 0x38, 0x19,
        0xEB, 0x4C, 0x77, 0x22, 0x01, 0x9D, 0x3C, 0xB7, 0x27, 0x5A, 0x23, 0xC8, 0x45, 0x0C, 0x88,
        0xB3, 0xB0, 0x02, 0x6A,
    ])
}

/// Overwrites the signature in `file_bytes` with zeroes.
pub fn zero_signature(file_bytes: &mut [u8]) -> Result<()> {
    const SIGNATURE_END: usize = SIGNATURE_OFFSET + SIGNATURE_LENGTH;
    if file_bytes.len() < SIGNATURE_END {
        bail!("Input file is too short to contain a signature");
    }
    file_bytes[SIGNATURE_OFFSET..SIGNATURE_END].fill(0);
    Ok(())
}

/// Compute the digest of the payload image with the required length
/// extention for later signing.
pub fn hash_image_bytes(payload: &[u8]) -> Result<[u8; 32]> {
    let header = ImageHeader::from_bytes(payload)
        .ok_or_else(|| anyhow!("Input file is too small to contain a header"))?;

    if header.taint != 0 {
        bail!("Input file is tainted (contains development code), refusing to sign");
    }

    let mut slot_length = header.slot_length as usize;
    // Old stage1 binaries didn't include the slot length in their header, they
    // had a reserved field with a value of 0. We shouldn't encounter them, but
    // if we do, assume a 64KB slot length as was used at that time.
    if slot_length == 0 {
        slot_length = 64 * 1024;
    }
    if payload.len() > slot_length {
        bail!(
            "Input file is too large to fit in stage1 slot. {} > {}",
            payload.len(),
            slot_length
        );
    }
    let v = extend_image(payload, slot_length);

    let mut hasher = Hash::new();
    for block in v.chunks(FLASH_PAGE_SZ) {
        hasher.update(block);
    }
    let hash = hasher.finalize();
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
    use mcu_common::ImageHeader;
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
        let mut image: [u8; super::FLASH_PAGE_SZ + OFFSET] = [0xCC; super::FLASH_PAGE_SZ + OFFSET];
        let mut header = ImageHeader::empty();
        header.slot_length = 4096;
        const HEADER_LEN: usize = core::mem::size_of::<ImageHeader>();
        image[..HEADER_LEN].copy_from_slice(unsafe {
            core::slice::from_raw_parts(&header as *const ImageHeader as *const u8, HEADER_LEN)
        });

        let ext_img_digest = super::hash_image_bytes(&image).unwrap();
        assert_eq!(ext_img_digest.len(), 32);
        // Confirm hash of sample data is invariant.
        assert_eq!(
            ext_img_digest,
            [
                143, 14, 216, 170, 204, 167, 53, 181, 32, 31, 217, 206, 98, 156, 202, 4, 240, 217,
                29, 192, 69, 12, 77, 49, 196, 10, 197, 224, 170, 93, 44, 51
            ]
        );
    }

    #[test]
    fn test_it_refuses_to_sign_tainted_header() {
        let mut image: [u8; 256] = [0xCC; 256];
        let mut header = ImageHeader::tainted();
        header.slot_length = 4096;
        const HEADER_LEN: usize = core::mem::size_of::<ImageHeader>();
        image[..HEADER_LEN].copy_from_slice(unsafe {
            core::slice::from_raw_parts(&header as *const ImageHeader as *const u8, HEADER_LEN)
        });

        assert!(super::hash_image_bytes(&image).is_err());
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

    #[test]
    fn test_zero_signature() {
        let mut buffer = [99u8; 100];
        super::zero_signature(&mut buffer).unwrap();
        assert_eq!(buffer[0], 99);
        assert_eq!(buffer[mcu_common::SIGNATURE_OFFSET], 0);
        assert_eq!(
            buffer[mcu_common::SIGNATURE_OFFSET + mcu_common::SIGNATURE_LENGTH],
            99
        );
    }
}
