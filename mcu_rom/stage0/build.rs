// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#[cfg(not(feature = "sys_bootloader"))]
fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
/*  FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K*/
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 64K
    HDR   (rx) : ORIGIN = 0x08010000, LENGTH = 128
    SLOT  (rx) : ORIGIN = 0x08010000, LENGTH = 64K
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}

PROVIDE(_hdr = ORIGIN(HDR));
PROVIDE(_slot_start = ORIGIN(SLOT));
PROVIDE(_slot_end   = ORIGIN(SLOT) + LENGTH(SLOT));
"#
}

/// Configuration if we're being loaded by the system bootloader. In this case,
/// we leave the first 1KB of flash empty so that the system bootloader still
/// runs.
#[cfg(feature = "sys_bootloader")]
fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
/*  FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K*/
    FLASH (rx) : ORIGIN = 0x08000400, LENGTH = 63K
    HDR   (rx) : ORIGIN = 0x08010000, LENGTH = 128
    SLOT  (rx) : ORIGIN = 0x08010000, LENGTH = 64K
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}

PROVIDE(_hdr = ORIGIN(HDR));
PROVIDE(_slot_start = ORIGIN(SLOT));
PROVIDE(_slot_end   = ORIGIN(SLOT) + LENGTH(SLOT));
"#
}

fn main() {
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
}
