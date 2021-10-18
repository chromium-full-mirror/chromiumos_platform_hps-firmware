// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    build_mcu_test_program();
}

fn build_mcu_test_program() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("factory_tester_mcu.bin");
    let out = Command::new("cargo")
        .current_dir("../factory_tester_mcu")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("objcopy")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--")
        .arg("-O")
        .arg("binary")
        // If we don't remove these sections, then LLVM's objcopy includes them
        // in the .bin file. So we end up with 10KB of zeros at the start of the
        // file. GNU's objcopy doesn't do this.
        .arg("--remove-section=.bss")
        .arg("--remove-section=.uninit")
        .arg(bin_file)
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
}

fn get_env_var(var: &str) -> String {
    match std::env::var(var) {
        Ok(x) => x,
        Err(_) => panic!("Environment variable {} undefined", var),
    }
}
