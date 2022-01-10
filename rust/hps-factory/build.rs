// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    build_mcu_test_program("factory_tester_mcu.bin", "");
    build_mcu_test_program("factory_tester_mcu_stage1.bin", "stage1");
    write_application_bin();
    write_release_application_bin();
    build_soc_rom();
    write_stage0_bin("stage0.bin", "");
    write_stage0_bin("stage0.sys.bin", "sys_bootloader");
    build_one_time_init();
}

fn build_mcu_test_program(bin_name: &str, features: &str) {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join(bin_name);
    let out = Command::new("cargo")
        .current_dir("../mcu/factory_tester_mcu")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg(features)
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    convert_elf_to_bin(
        "../mcu/target/thumbv6m-none-eabi/release/factory_tester_mcu",
        &bin_file,
    );
}

fn write_stage0_bin(bin_name: &str, features: &str) {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join(bin_name);
    let out = Command::new("cargo")
        .current_dir("../mcu/stage0")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg(features)
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    convert_elf_to_bin("../mcu/target/thumbv6m-none-eabi/release/stage0", &bin_file);
}

fn write_application_bin() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("application.bin");
    let out = Command::new("cargo")
        .current_dir("../mcu/stage1_app")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg("proto2,dev,image-transfer,no-hash-check")
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    convert_elf_to_bin(
        "../mcu/target/thumbv6m-none-eabi/release/stage1_app",
        &bin_file,
    );
    sign_stage1_binary(&bin_file);
}

fn write_release_application_bin() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("application.release.bin");
    let out = Command::new("cargo")
        .current_dir("../mcu/stage1_app")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        // TODO: Remove no-hash-check once stage1 embeds the hash of the Rust
        // fpga_rom rather than the old C soc_rom.
        .arg("proto2,no-hash-check")
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    convert_elf_to_bin(
        "../mcu/target/thumbv6m-none-eabi/release/stage1_app",
        &bin_file,
    );
    sign_stage1_binary(&bin_file);
}

fn build_one_time_init() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("one_time_init.bin");
    let out = Command::new("cargo")
        .current_dir("../mcu/one_time_init")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("build")
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    convert_elf_to_bin(
        "../mcu/target/thumbv6m-none-eabi/debug/one_time_init",
        &bin_file,
    );
}

fn build_soc_rom() {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bitstream = Path::new("../../build/hps_platform/gateware/hps_platform.bit");
    // Due to how slow it is to build the gateware, we only build if the output
    // files don't exist. i.e. rebuilding is left to the user.
    if !bitstream.exists() {
        let out = Command::new("../../scripts/build-fpga-bitstream")
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .output()
            .unwrap();
        if !out.status.success() {
            eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
            std::process::exit(1);
        }
    }
    copy_or_die(bitstream, out_dir.join("hps_platform.bit"));

    let fpga_rom = out_dir.join("soc_rom.bin");
    let out = Command::new("cargo")
        .current_dir("../riscv/fpga_rom")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg("image-transfer")
        .output()
        .unwrap();
    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    convert_elf_to_bin(
        "../riscv/target/riscv32i-unknown-none-elf/release/fpga_rom",
        &fpga_rom,
    );
}

fn copy_or_die(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    let from = from.as_ref();
    let to = to.as_ref();
    if let Err(error) = std::fs::copy(from, to) {
        panic!("Failed to copy {:?} to {:?}: {}", from, to, error);
    }
}

fn convert_elf_to_bin(src: impl AsRef<Path>, dst: impl AsRef<Path>) {
    let cmd = std::env::var("OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".to_string());
    let out = Command::new(cmd)
        .arg("-O")
        .arg("binary")
        .arg(src.as_ref())
        .arg(dst.as_ref())
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

fn sign_stage1_binary(bin_file: &Path) {
    let mut file_bytes = std::fs::read(bin_file).unwrap();
    sign_rom::zero_signature(&mut file_bytes).unwrap();
    let hash = sign_rom::hash_image_bytes(&file_bytes, mcu_common::STAGE1_SLOT_LENGTH).unwrap();
    let signature = sign_rom::sign_hash(&hash, &sign_rom::development_only_secret_key());
    sign_rom::write_signature(&mut file_bytes, &signature).unwrap();
    std::fs::write(bin_file, file_bytes).unwrap();
}
