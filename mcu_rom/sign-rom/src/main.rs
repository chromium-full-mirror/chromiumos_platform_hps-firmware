// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use mcu_common::HostImageHeader;
use mcu_common::PrivateKey;
use mcu_common::Signature;
use mcu_common::FLASH_PAGE_SZ;
use mcu_common::{sign_hash, Hash};
use std::path::PathBuf;

#[derive(FromArgs)]
/// Hashes and signs a HPS stage1 binary. This is currently intended for
/// development use only.
struct Config {
    #[argh(option)]
    /// input filename
    input: PathBuf,

    #[argh(option)]
    /// output filename
    output: PathBuf,

    #[argh(option)]
    /// if set, generates and prints a keypair using the supplied seed, which is
    /// a comma-separated list of u8 of length up to 32
    generate_keypair_seed: Option<String>,
}

// Note, this key is for development purposes only. It isn't and won't be used
// for production devices.
const SECRET_KEY: PrivateKey = PrivateKey::from_bytes([
    0x9B, 0x25, 0x37, 0xA1, 0xDB, 0x16, 0x5E, 0xDC, 0x62, 0x23, 0x52, 0x8A, 0xD7, 0x5B, 0xE4, 0x46,
    0xF7, 0x79, 0xDC, 0x6B, 0x62, 0x8D, 0x60, 0xF0, 0x5D, 0x96, 0x95, 0x61, 0xAF, 0x50, 0xCF, 0xD5,
    0x01, 0xB5, 0x9C, 0x83, 0xD3, 0x8D, 0x40, 0x9B, 0x71, 0x18, 0xA8, 0x38, 0x19, 0xEB, 0x4C, 0x77,
    0x22, 0x01, 0x9D, 0x3C, 0xB7, 0x27, 0x5A, 0x23, 0xC8, 0x45, 0x0C, 0x88, 0xB3, 0xB0, 0x02, 0x6A,
]);

fn main() -> Result<()> {
    run(&argh::from_env())
}

fn run(config: &Config) -> Result<()> {
    if let Some(seed) = &config.generate_keypair_seed {
        let mut sk = [0u8; sodalite::SIGN_SECRET_KEY_LEN];
        let mut pk = [0u8; sodalite::SIGN_PUBLIC_KEY_LEN];
        let mut seed_bytes = [0u8; 32];
        for (i, part) in seed.split(',').enumerate() {
            seed_bytes[i] = part.parse::<u8>()?;
        }
        sodalite::sign_keypair_seed(&mut pk, &mut sk, &seed_bytes);
        println!("Secret key: {:#02X?}", sk);
        println!("Public key: {:#02X?}", pk);
        return Ok(());
    }
    let mut file_bytes = std::fs::read(&config.input)?;
    sign_file_bytes(&mut file_bytes)?;
    std::fs::write(&config.output, file_bytes)?;
    Ok(())
}

/// Computes the signature of the non-header portion of `file_bytes` then stores
/// that signature into the appropriate location in the header at the start of
/// `file_bytes`
fn sign_file_bytes(file_bytes: &mut [u8]) -> Result<()> {
    let (signature_out, payload) = split_signature_and_payload(file_bytes)?;
    // FIXME(quasisec): This constant should be passed in as a argument to main().
    let len = 0x1C000; // FLASH (rx) : ORIGIN = 0x08004080, LENGTH = 112K /* - LENGTH(HDR) */
    let v = extend_image(&payload, len);

    let mut hasher = Hash::new();
    for block in v.chunks(FLASH_PAGE_SZ) {
        hasher.update(block);
    }
    let hash = hasher.finalize();
    println!("signing with digest = {:#02X?}", hash);
    let signature = sign_hash(&hash, &SECRET_KEY);
    println!("injecting signature = {:#02X?}", signature.raw_bytes());

    signature_out.copy_from(&signature);
    Ok(())
}

/// Returns a mutable reference to the signature as well as the payload (the
/// data that the signature applies to).
fn split_signature_and_payload(file_bytes: &mut [u8]) -> Result<(&mut Signature, &[u8])> {
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

/// Return a extended image with a provided length.
fn extend_image(image: &[u8], extend_length: usize) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&image);

    // Determine if the image is able to precisely divide
    // into page alignment and if not pad.
    if v.len() / FLASH_PAGE_SZ != 0 {
        // Determine the residue of remaining sectors
        // that will complete the page and pad with
        // zeros.
        let rem = v.len() % (FLASH_PAGE_SZ / 256);
        v.resize(v.len() + rem, 0x00);
    }
    // Further extend the image to fill the entire region length
    // provided by padding the remaining pages with 0xFF.
    v.resize(extend_length, 0xFF);

    v
}

#[cfg(test)]
mod test {
    #[test]
    fn test_extend_image() {
        // offset image by 32+1 Bytes to simulate mis-alignment.
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
