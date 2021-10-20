// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    build_mcu_test_program();
    write_application_bin();
    build_soc_rom();
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

fn write_application_bin() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("application.bin");
    let out = Command::new("cargo")
        .current_dir("../stage1_app")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("objcopy")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg("proto2,image-transfer")
        .arg("--")
        .arg("-O")
        .arg("binary")
        .arg(bin_file)
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
}

fn build_soc_rom() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bitstream = Path::new("../../build/hps_platform/gateware/hps_platform.bit");
    let soc_rom = Path::new("../../build/hps_platform/software/bios/bios.bin");
    // Due to how slow it is to build the gateware, we only build if the output
    // files don't exist. i.e. rebuilding is left to the user.
    if !bitstream.exists() || !soc_rom.exists() {
        let out = Command::new("../../scripts/soc-load")
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .arg("--build")
            .output()
            .unwrap();
        if !out.status.success() {
            eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
            std::process::exit(1);
        }
    }
    copy_or_die(bitstream, out_dir.join("hps_platform.bit"));
    copy_or_die(soc_rom, out_dir.join("soc_rom.bin"));
}

fn copy_or_die(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    let from = from.as_ref();
    let to = to.as_ref();
    if let Err(error) = std::fs::copy(from, to) {
        panic!("Failed to copy {:?} to {:?}: {}", from, to, error);
    }
}

fn get_env_var(var: &str) -> String {
    match std::env::var(var) {
        Ok(x) => x,
        Err(_) => panic!("Environment variable {} undefined", var),
    }
}
