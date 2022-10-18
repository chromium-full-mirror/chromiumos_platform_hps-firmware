// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use mcu_common::MCU_RAM_SIZE;
use mcu_common::MCU_RAM_START;
use mcu_common::PROGRAM_IN_RAM_OFFSET;

fn main() {
    write_memory_x();
}

fn memory_x_contents() -> String {
    format!(
        r#"MEMORY
{{
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    RAM  (xrw) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
}}
"#,
        MCU_RAM_START + PROGRAM_IN_RAM_OFFSET,
        MCU_RAM_SIZE - PROGRAM_IN_RAM_OFFSET,
        MCU_RAM_START,
        PROGRAM_IN_RAM_OFFSET
    )
}

fn write_memory_x() {
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
