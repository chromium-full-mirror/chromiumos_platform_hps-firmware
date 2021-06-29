// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use mcu_common::Hash;
use mcu_common::Signature;
use sodalite::HASH_LEN;
use std::path::PathBuf;

#[derive(FromArgs)]
/// Hashes and signs a HPS stage1 binary. This is currently intended for
/// development use only.
struct Config {
    #[argh(option)]
    /// input filename
    input: PathBuf,

    #[argh(option)]
    /// input filename region length
    region_length: usize,

    #[argh(option)]
    /// input signature
    signature: Option<String>,

    #[argh(option)]
    /// output filename
    output: Option<PathBuf>,

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

fn main() -> Result<()> {
    let config: Config = argh::from_env();

    // Handy debug keypair generaton.
    generate_debug_keypair(&config)?;

    let mut file_bytes = std::fs::read(&config.input)?;
    if config.region_length < file_bytes.len() {
        bail!("Input file is too big to fit into specified region length");
    }
    println!("Using region length = 0x{:04X}", config.region_length);

    // Computes the signature of the non-header portion of `file_bytes` then stores
    // that signature into the appropriate location in the header at the start of
    // `file_bytes`
    let (signature_out, payload) = sign_rom::split_signature_and_payload(&mut file_bytes)?;
    let hash = sign_rom::hash_image_bytes(payload, config.region_length)?;
    println!("Calculated image digest = {:?}", hash.as_bytes());

    let signature = if let Some(sig) = &config.signature {
        let mut signature_bytes = [0u8; 64];
        for (i, part) in sig.split(',').enumerate() {
            signature_bytes[i] = part.parse::<u8>()?;
        }
        Signature::empty() // TODO(quasisec): construct a signature with provided data.
    } else {
        sign_hash(&hash, &SECRET_KEY)
    };

    // If a path to output file is provided, write out image with modified
    // header containing signature.
    if let Some(output) = &config.output {
        println!("injecting signature = {:#02X?}", signature.raw_bytes());
        signature_out.copy_from(&signature);

        std::fs::write(&output, file_bytes)?;
    }

    Ok(())
}

/// generate_debug_keypair allows for local development of signing debug
/// images. Given a seed value as a slice a key pair is printed in return.
fn generate_debug_keypair(config: &Config) -> Result<()> {
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
    } else {
        return Ok(());
    }
}

fn sign_hash(hash: &Hash, private_key: &PrivateKey) -> Signature {
    let mut out = [0u8; 64 + HASH_LEN]; /* sign_len=64 */
    sodalite::sign_attached(&mut out, &hash.0, &private_key.raw_bytes);
    let mut signature = Signature {
        raw_bytes: [0u8; 64],
    };
    signature.raw_bytes.clone_from_slice(&out[..64]);
    signature
}

#[cfg(test)]
mod tests {
    use mcu_common::FLASH_PAGE_SZ;
    use sodalite::SIGN_SECRET_KEY_LEN;

    #[test]
    fn test_signing() {
        let private_key = super::PrivateKey::from_bytes([42u8; crate::SIGN_SECRET_KEY_LEN]);

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
}
