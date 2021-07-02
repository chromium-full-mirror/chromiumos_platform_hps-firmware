// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use mcu_common::Hash;
use mcu_common::HostImageHeader;
use mcu_common::Signature;
use mcu_common::{FLASH_PAGE_SZ, FLASH_WRITE_SZ};
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
pub fn hash_image_bytes(payload: &[u8], region_length: usize) -> Result<Hash> {
    let v = extend_image(&payload, region_length);

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
    v.extend_from_slice(&image);

    // Determine the residue of remaining Bytes that
    // will complete the write size and pad with zeros.
    let rem = v.len() % FLASH_WRITE_SZ;
    v.resize(v.len() + rem, 0x00);

    // Further extend the image to fill the entire region length
    // provided by padding the remaining pages with 0xFF.
    v.resize(extend_length, 0xFF);

    v
}

pub fn sign_hash(hash: &Hash, private_key: &PrivateKey) -> Signature {
    let mut out = [0u8; 64 + sodalite::HASH_LEN]; /* sign_len=64 */
    sodalite::sign_attached(&mut out, &hash.0, &private_key.raw_bytes);
    let mut signature = Signature {
        raw_bytes: [0u8; 64],
    };
    signature.raw_bytes.clone_from_slice(&out[..64]);
    signature
}

#[cfg(test)]
mod test {
    use sodalite::SIGN_SECRET_KEY_LEN;

    #[test]
    fn test_signing() {
        let private_key = super::PrivateKey::from_bytes([42u8; SIGN_SECRET_KEY_LEN]);

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

    #[test]
    fn test_hash_image_bytes() {
        // Ensure data size isn't an exact multiple of the flash write size (8) so that
        // zero-padding is tested.
        const OFFSET: usize = 32 + 1;
        let image: [u8; super::FLASH_PAGE_SZ + OFFSET] = [0xCC; super::FLASH_PAGE_SZ + OFFSET];
        let ext_len: usize = 4096;

        let ext_img_digest = super::hash_image_bytes(&image, ext_len).unwrap();
        assert_eq!(ext_img_digest.as_bytes().len(), 64);
        // Confirm hash of sample data is invariant.
        assert_eq!(
            ext_img_digest.as_bytes(),
            [
                120, 205, 118, 249, 217, 204, 106, 182, 60, 233, 208, 229, 74, 235, 12, 243, 4, 10,
                135, 89, 111, 111, 59, 202, 92, 242, 19, 239, 79, 177, 65, 70, 246, 65, 198, 226,
                30, 240, 228, 13, 98, 117, 90, 10, 158, 75, 66, 14, 192, 82, 67, 185, 94, 107, 0,
                193, 0, 148, 246, 212, 153, 106, 199, 60
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
