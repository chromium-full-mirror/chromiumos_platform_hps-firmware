// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    build_mcu_test_program("factory_tester_mcu.bin", "")?;
    build_mcu_test_program("factory_tester_mcu_stage1.bin", "stage1")?;
    write_application_bin()?;
    write_release_application_bin()?;
    build_soc_rom()?;
    write_stage0_bin("stage0.bin", "")?;
    write_stage0_bin("stage0.sys.bin", "sys_bootloader")?;
    build_one_time_init()?;
    Ok(())
}

fn build_mcu_test_program(bin_name: &str, features: &str) -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join(bin_name);
    Command::new("cargo")
        .current_dir("../mcu/factory_tester_mcu")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg(features)
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/release/factory_tester_mcu"),
        &bin_file,
    )
}

fn write_stage0_bin(bin_name: &str, features: &str) -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join(bin_name);
    Command::new("cargo")
        .current_dir("../mcu/stage0")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg(features)
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/release/stage0"),
        &bin_file,
    )
}

fn write_application_bin() -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("application.bin");
    Command::new("cargo")
        .current_dir("../mcu/stage1_app")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg("proto2,dev,image-transfer,no-hash-check")
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/release/stage1_app"),
        &bin_file,
    )?;
    sign_stage1_binary(&bin_file)
}

fn write_release_application_bin() -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("application.release.bin");
    Command::new("cargo")
        .current_dir("../mcu/stage1_app")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        // TODO: Remove no-hash-check once stage1 embeds the hash of the Rust
        // fpga_rom rather than the old C soc_rom.
        .arg("proto2,no-hash-check")
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/release/stage1_app"),
        &bin_file,
    )?;
    sign_stage1_binary(&bin_file)
}

fn build_one_time_init() -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join("one_time_init.bin");
    Command::new("cargo")
        .current_dir("../mcu/one_time_init")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/debug/one_time_init"),
        &bin_file,
    )
}

fn build_soc_rom() -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bitstream = Path::new("../../build/hps_platform/gateware/hps_platform.bit");
    // Due to how slow it is to build the gateware, we only build if the output
    // files don't exist. i.e. rebuilding is left to the user.
    if !bitstream.exists() {
        Command::new("../../scripts/build-fpga-bitstream").run()?;
    }
    copy_or_die(bitstream, out_dir.join("hps_platform.bit"));

    let fpga_rom = out_dir.join("soc_rom.bin");
    Command::new("cargo")
        .current_dir("../riscv/fpga_rom")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg("image-transfer")
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/riscv32i-unknown-none-elf/release/fpga_rom"),
        &fpga_rom,
    )
}

fn copy_or_die(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    let from = from.as_ref();
    let to = to.as_ref();
    if let Err(error) = std::fs::copy(from, to) {
        panic!("Failed to copy {:?} to {:?}: {}", from, to, error);
    }
}

fn convert_elf_to_bin(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let cmd = std::env::var("OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".to_string());
    Command::new(cmd)
        .arg("-O")
        .arg("binary")
        .arg(src.as_ref())
        .arg(dst.as_ref())
        .run()?;
    Ok(())
}

fn get_env_var(var: &str) -> String {
    match std::env::var(var) {
        Ok(x) => x,
        Err(_) => panic!("Environment variable {} undefined", var),
    }
}

fn sign_stage1_binary(bin_file: &Path) -> Result<()> {
    let mut file_bytes = std::fs::read(bin_file)
        .with_context(|| format!("Failed to open {} for signing", bin_file.display()))?;
    sign_rom::zero_signature(&mut file_bytes)?;
    let hash = sign_rom::hash_image_bytes(&file_bytes, mcu_common::STAGE1_SLOT_LENGTH)?;
    let signature = sign_rom::sign_hash(&hash, &sign_rom::development_only_secret_key());
    sign_rom::write_signature(&mut file_bytes, &signature)?;
    std::fs::write(bin_file, file_bytes)?;
    Ok(())
}

trait RunCommand {
    fn run(&mut self) -> Result<()>;
}

impl RunCommand for Command {
    fn run(&mut self) -> Result<()> {
        let out = self
            .output()
            .with_context(|| format!("Failed to launch command: {:?}", self))?;
        if !out.status.success() {
            eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
            eprintln!("{}", std::str::from_utf8(&out.stdout).unwrap());
            bail!("Command returned non-zero status: {:?}", self);
        }
        Ok(())
    }
}
