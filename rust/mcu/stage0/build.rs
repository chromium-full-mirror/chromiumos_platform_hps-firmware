// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Context;
use anyhow::Result;

#[cfg(not(feature = "sys_bootloader"))]
const STAGE0_FLASH_START: u32 = 0x08000000;

/// When we're being loaded by the system bootloader, we leave the first 1KB of
/// flash empty so that the system bootloader still runs.
#[cfg(feature = "sys_bootloader")]
const STAGE0_FLASH_START: u32 = 0x08000400;

fn memory_x_contents() -> String {
    // The full extent of the flash is:
    // FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K
    //
    // Note, the MCU actually has 36KB of RAM. We restrict ourselves to 35KB so
    // that stage1 has the option to write crash information into the last 1KB
    // and have it remain on the next boot.
    format!(
        r#"MEMORY
{{
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = {}
    SLOT  (rx) : ORIGIN = 0x{:x}, LENGTH = {}
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 35K
    OTP (r)    : ORIGIN = 0x1FFF7000, LENGTH = 1K
}}

PROVIDE(_slot = ORIGIN(SLOT));
PROVIDE(OTP = ORIGIN(OTP));
"#,
        STAGE0_FLASH_START,
        mcu_common::FLASH_SIZE - mcu_common::STAGE1_SLOT_LENGTH as u32,
        mcu_common::APPLICATION_START_ADDRESS,
        mcu_common::STAGE1_SLOT_LENGTH,
    )
}

fn write_raw_public_key<P: AsRef<std::path::Path>>(pem_path: P) -> Result<()> {
    let pem_key = std::fs::read_to_string(pem_path.as_ref())
        .with_context(|| format!("Failed to read PEM public key from {:?}", pem_path.as_ref()))?;
    let key = ed25519_compact::PublicKey::from_pem(&pem_key).with_context(|| {
        format!(
            "Failed to parse PEM public key from {:?}",
            pem_path.as_ref()
        )
    })?;
    let key_bytes: [u8; ed25519_compact::PublicKey::BYTES] = *key;
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::write(out_dir.join("raw-public-key"), key_bytes)?;
    println!(
        "cargo:rerun-if-changed={}",
        pem_path.as_ref().to_str().unwrap()
    );
    Ok(())
}

fn main() -> Result<()> {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let memory_x_filename = out_dir.join("memory.x");
    let contents = memory_x_contents();
    // We only write memory.x if it doesn't exist or needs changing, otherwise
    // we'll trigger a rebuild even if no files have changed.
    if std::fs::read_to_string(&memory_x_filename).unwrap_or_default() != contents {
        if let Err(error) = std::fs::write(&memory_x_filename, memory_x_contents()) {
            panic!("Failed to write {}: {}", memory_x_filename.display(), error);
        }
    }
    // Tell cargo to look for memory.x in our out dir.
    println!("cargo:rustc-link-search={}", out_dir.display());
    // Tell cargo to rebuild if memory.x has changed.
    println!("cargo:rerun-if-changed=memory.x");

    // Exactly one '*-key' feature must be enabled when building this crate,
    // otherwise you will get a compile error on the lines below.
    let pem_path = {
        #[cfg(feature = "insecure-dev-key")]
        {
            "keys/dev.pub.pem"
        }
        #[cfg(feature = "premp-key")]
        {
            "keys/hps-accessory-premp.pub.pem"
        }
        #[cfg(feature = "mp-key")]
        {
            "keys/hps-accessory-mp.pub.pem"
        }
    };
    write_raw_public_key(pem_path)?;

    Ok(())
}
