// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    write_memory_x();
    build_mcu_test_program();
}

fn build_mcu_test_program() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("factory_tester_mcu.bin");
    let out = Command::new("cargo")
        .current_dir("../mcu/factory_tester_mcu")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    let out = Command::new(std::env::var("OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".to_string()))
        .arg("-O")
        .arg("binary")
        .arg(out_dir.join("nested-build/thumbv6m-none-eabi/release/factory_tester_mcu"))
        .arg(bin_file)
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
}

fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 512K
    RAM  (rw) : ORIGIN = 0x20000000, LENGTH = 160K
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

fn get_env_var(var: &str) -> String {
    match std::env::var(var) {
        Ok(x) => x,
        Err(_) => panic!("Environment variable {} undefined", var),
    }
}
