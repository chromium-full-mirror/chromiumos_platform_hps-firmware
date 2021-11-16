// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#[cfg(not(feature = "sys_bootloader"))]
const STAGE0_FLASH_START: u32 = 0x08000000;

/// When we're being loaded by the system bootloader, we leave the first 1KB of
/// flash empty so that the system bootloader still runs.
#[cfg(feature = "sys_bootloader")]
const STAGE0_FLASH_START: u32 = 0x08000400;

fn memory_x_contents() -> String {
    // The full extent of the flash is:
    // FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K
    format!(
        r#"MEMORY
{{
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = 64K
    SLOT  (rx) : ORIGIN = 0x{:x}, LENGTH = {}
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}}

PROVIDE(_slot = ORIGIN(SLOT));
"#,
        STAGE0_FLASH_START,
        mcu_common::APPLICATION_START_ADDRESS,
        mcu_common::STAGE1_SLOT_LENGTH,
    )
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
