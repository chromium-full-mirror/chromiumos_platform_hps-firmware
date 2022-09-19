// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use mcu_common::Signature;
use mcu_common::SIGNATURE_LENGTH;
use std::path::PathBuf;

/// Hashes and signs a HPS stage1 binary. This is currently intended for
/// development use only.
#[derive(Debug, Parser)]
#[clap(name = "config")]
struct Config {
    /// input filename
    #[clap(short, long)]
    input: PathBuf,

    /// input signature
    #[clap(short, long)]
    signature: Option<String>,

    /// private key filename
    #[clap(long, required_unless_present("use-insecure-dev-key"))]
    private_key: Option<PathBuf>,

    /// sign with publicly-disclosed development key
    #[clap(long, conflicts_with = "private-key")]
    use_insecure_dev_key: bool,

    /// output filename
    #[clap(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let config = Config::parse();

    let mut file_bytes = std::fs::read(&config.input)?;

    // Replace any existing signature with zeroes before computing the digest.
    // This ensures the operation is idempotent, and that we can re-sign an image
    // using a different key and get the correct result.
    sign_rom::zero_signature(&mut file_bytes)?;

    // Computes the signature of the non-header portion of `file_bytes` then stores
    // that signature into the appropriate location in the header at the start of
    // `file_bytes`
    let hash = sign_rom::hash_image_bytes(&file_bytes)?;
    println!("Calculated image digest = {:02X?}", hash);

    // If a path to output file is provided, write out image with modified
    // header containing signature.
    if let Some(output) = &config.output {
        let signature = get_signature(&config, &hash)?;
        println!("injecting signature = {:02X?}", signature.raw_bytes());
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
        let secret_key = get_secret_key(config)?;
        Ok(sign_rom::sign_hash(hash, &secret_key))
    }
}

fn get_secret_key(config: &Config) -> Result<ed25519_compact::SecretKey> {
    if config.use_insecure_dev_key {
        Ok(sign_rom::development_only_secret_key())
    } else {
        let pem_key = std::fs::read_to_string(config.private_key.as_ref().unwrap())?;
        ed25519_compact::SecretKey::from_pem(&pem_key)
            .map_err(|error| anyhow!("Failed to parse private key file: {}", error))
    }
}
