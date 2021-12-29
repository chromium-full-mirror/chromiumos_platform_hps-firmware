// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use clap::Clap;

use anyhow::bail;
use anyhow::Result;
use mcu_common::Signature;
use mcu_common::SIGNATURE_LENGTH;
use std::path::PathBuf;

/// Hashes and signs a HPS stage1 binary. This is currently intended for
/// development use only.
#[derive(Clap, Debug)]
#[clap(name = "config")]
struct Config {
    /// input filename
    #[clap(short, long)]
    input: PathBuf,

    /// input filename region length
    #[clap(short, long, default_value = "65536")]
    region_length: usize,

    /// input signature
    #[clap(short, long)]
    signature: Option<String>,

    /// output filename
    #[clap(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let config = Config::parse();

    let mut file_bytes = std::fs::read(&config.input)?;
    if config.region_length < file_bytes.len() {
        bail!("Input file is too big to fit into specified region length");
    }
    println!("Using region length = 0x{:04X}", config.region_length);

    // Replace any existing signature with zeroes before computing the digest.
    // This ensures the operation is idempotent, and that we can re-sign an image
    // using a different key and get the correct result.
    sign_rom::zero_signature(&mut file_bytes)?;

    // Computes the signature of the non-header portion of `file_bytes` then stores
    // that signature into the appropriate location in the header at the start of
    // `file_bytes`
    let hash = sign_rom::hash_image_bytes(&file_bytes, config.region_length)?;
    println!("Calculated image digest = {:?}", hash);

    // If a path to output file is provided, write out image with modified
    // header containing signature.
    if let Some(output) = &config.output {
        let signature = get_signature(&config, &hash)?;
        println!("injecting signature = {:#02X?}", signature.raw_bytes());
        sign_rom::write_signature(&mut file_bytes, &signature)?;

        std::fs::write(&output, file_bytes)?;
    }

    Ok(())
}

fn get_signature(config: &Config, hash: &[u8]) -> Result<Signature> {
    if let Some(sig) = &config.signature {
        let mut signature_bytes = [0u8; SIGNATURE_LENGTH];
        for (i, part) in sig.split(',').enumerate() {
            signature_bytes[i] = part.parse::<u8>()?;
        }
        Ok(Signature {
            raw_bytes: signature_bytes,
        })
    } else {
        // Note, this key is for development purposes only. It isn't and won't be used
        // for production devices.
        let secret_key: ed25519_compact::SecretKey = ed25519_compact::SecretKey::new([
            0x9B, 0x25, 0x37, 0xA1, 0xDB, 0x16, 0x5E, 0xDC, 0x62, 0x23, 0x52, 0x8A, 0xD7, 0x5B,
            0xE4, 0x46, 0xF7, 0x79, 0xDC, 0x6B, 0x62, 0x8D, 0x60, 0xF0, 0x5D, 0x96, 0x95, 0x61,
            0xAF, 0x50, 0xCF, 0xD5, 0x01, 0xB5, 0x9C, 0x83, 0xD3, 0x8D, 0x40, 0x9B, 0x71, 0x18,
            0xA8, 0x38, 0x19, 0xEB, 0x4C, 0x77, 0x22, 0x01, 0x9D, 0x3C, 0xB7, 0x27, 0x5A, 0x23,
            0xC8, 0x45, 0x0C, 0x88, 0xB3, 0xB0, 0x02, 0x6A,
        ]);
        Ok(sign_rom::sign_hash(hash, &secret_key))
    }
}
