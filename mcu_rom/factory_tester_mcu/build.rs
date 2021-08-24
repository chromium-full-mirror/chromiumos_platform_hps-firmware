// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

fn main() {
    write_memory_x();
}

// In our production config, we completely avoid using the MCU's flash. Our
// "flash" is actually RAM. We just call it "FLASH" for compatibility with the
// upstream-provided linker script. The factory bootloader uses the first 0x2700
// bytes of RAM, so our "FLASH" cannot overlap with this space. Our RAM can
// overlap wit the space, since we don't write it until after we've started, at
// which point the factory bootloader doesn't need whatever it wrote there. We
// round 0x2700 up to the next whole KB and start our "FLASH" at 0x2800.
#[cfg(not(feature = "dev"))]
fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
    FLASH (rx) : ORIGIN = 0x20002800, LENGTH = 26K
    RAM  (rw) : ORIGIN = 0x20000000, LENGTH = 10K
}
"#
}

// Our dev-mode configuration. In this case we're not being loaded by the
// factory bootloader, we're just being flashed with openocd or similar, so we
// use actual flash. The dev mode configuration has more flash and RAM available
// to it, which allows us to put heaps of debug logging in.
#[cfg(feature = "dev")]
fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}
"#
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
