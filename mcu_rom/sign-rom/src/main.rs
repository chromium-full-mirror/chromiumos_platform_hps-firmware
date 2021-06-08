// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use mcu_common::ImageHeader;
use mcu_common::PrivateKey;
use mcu_common::Signature;
use mcu_common::FLASH_PAGE_SZ;
use mcu_common::SIGN_SECRET_KEY_LEN;
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
const SECRET_KEY: PrivateKey = PrivateKey::from_bytes([0u8; SIGN_SECRET_KEY_LEN]);

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

    let mut v = Vec::new();
    v.extend_from_slice(&payload);
    if v.len() / FLASH_PAGE_SZ != 0 {
        let rem = v.len() % (FLASH_PAGE_SZ / 256);
        v.resize(v.len() + rem, 0x00);
    }
    let len = 0x1C000; // FLASH (rx) : ORIGIN = 0x08004080, LENGTH = 112K /* - LENGTH(HDR) */
    v.resize(len, 0xFF);

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
    if file_bytes.len() < std::mem::size_of::<ImageHeader>() {
        bail!("Input file is too short to contain ImageHeader");
    }
    // Note, this is possibly technically invalid, since ImageHeder contains
    // references that won't be valid in the current memory space. We don't use
    // those references, but technically Rust would be permitted to dereference
    // these references. We also don't check that file_bytes is properly aligned
    // for an ImageHader, but again we don't use any fields that have alignment
    // more than 1.
    let header = unsafe { &mut *(&mut file_bytes[0] as *mut u8 as *mut ImageHeader) };
    Ok((&mut header.sig, &file_bytes[..]))
}

#[cfg(test)]
mod test {
    #[test]
    fn test_sign_file_bytes() {
        let mut file_bytes = [42u8; 100];
        super::sign_file_bytes(&mut file_bytes).unwrap();
        assert_eq!(
            file_bytes,
            [
                42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
                42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 128, 41, 109, 242, 194, 3, 102, 93,
                159, 1, 28, 128, 233, 17, 141, 214, 113, 189, 73, 205, 181, 228, 97, 180, 245, 44,
                181, 189, 199, 25, 116, 236, 112, 140, 130, 101, 17, 36, 42, 59, 133, 149, 71, 83,
                175, 210, 72, 63, 168, 41, 227, 10, 170, 1, 0, 187, 7, 162, 199, 164, 58, 219, 60,
                14, 42, 42, 42, 42
            ]
        );
    }

    #[test]
    fn test_input_too_short() {
        let mut file_bytes = [42u8; 10];
        assert!(super::sign_file_bytes(&mut file_bytes).is_err());
    }
}
