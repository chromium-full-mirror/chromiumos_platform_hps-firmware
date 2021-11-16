// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

fn main() {
    write_memory_x();
}

fn memory_x_contents() -> String {
    mcu_common::build_utils::stage1_linker_script()
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
