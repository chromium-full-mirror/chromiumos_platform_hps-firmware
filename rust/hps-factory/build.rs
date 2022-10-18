// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    copy_fpga_bitstream()?;
    build_fpga_rom()?;
    write_application_bin()?;
    write_stage0_bin("stage0.bin", "mp-key")?;
    build_one_time_init_loader("one_time_init_loader.bin", "")?;
    build_one_time_init_loader("one_time_init_loader_legacy.bin", "legacy-stage0")?;
    Ok(())
}

fn write_stage0_bin(bin_name: &str, features: &str) -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join(bin_name);
    if !cfg!(feature = "stage0-from-source") {
        copy_or_die("../../bin/stage0/v4.bin", &bin_file);
        return Ok(());
    }
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
    if !cfg!(feature = "built-in-mcu-rom") {
        std::fs::write(bin_file, "")?;
        return Ok(());
    }
    let mut features = "proto2".to_owned();
    if cfg!(feature = "image-transfer") {
        // The dev feature is required for image-transfer.
        features.push_str(",dev,image-transfer");
    }
    Command::new("cargo")
        .current_dir("../mcu/stage1_app")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .env("HPS_SPI_BIN", out_dir.join("soc_rom.bin"))
        .arg("build")
        .arg("--release")
        .arg("--no-default-features")
        .arg("--features")
        .arg(&features)
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/release/stage1_app"),
        &bin_file,
    )?;
    // We need to write a non-zero version into our stage1 binary so that
    // hps-factory can skip writing when the version is up-to-date, but rewrite
    // it when it's not.
    write_version_to_stage1_binary(&bin_file)
}

fn build_one_time_init_loader(name: &str, features: &str) -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bin_file = out_dir.join(name);
    Command::new("cargo")
        .current_dir("../mcu/one_time_init_loader")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .arg("--features")
        .arg(features)
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/debug/one_time_init_loader"),
        &bin_file,
    )?;
    sign_stage1_binary(&bin_file)
}

fn copy_fpga_bitstream() -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let bitstream_dest = out_dir.join("hps_platform.bit");
    if cfg!(feature = "built-in-fpga-bitstream") {
        let bitstream = Path::new("../../build/hps_platform/gateware/hps_platform.bit");
        // Due to how slow it is to build the gateware, we only complain if the output
        // files don't exist. i.e. rebuilding is left to the user.
        if !bitstream.exists() {
            bail!(
                "FPGA bitstream not found at {:?}\n\
                 (run scripts/build-fpga-bitstream inside the chroot to build it)",
                bitstream
            );
        }
        copy_or_die(bitstream, bitstream_dest);
    } else {
        std::fs::write(bitstream_dest, &[])?;
    }
    Ok(())
}

fn build_fpga_rom() -> Result<()> {
    let out_dir = PathBuf::from(get_env_var("OUT_DIR"));
    let fpga_rom = out_dir.join("soc_rom.bin");
    if cfg!(feature = "built-in-fpga-rom") {
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
    } else {
        std::fs::write(fpga_rom, &[])?;
        Ok(())
    }
}

fn copy_or_die(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    let from = from.as_ref();
    let to = to.as_ref();
    if let Err(error) = std::fs::copy(from, to) {
        panic!("Failed to copy {:?} to {:?}: {}", from, to, error);
    }
}

fn convert_elf_to_bin(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let mut objcopy = "llvm-objcopy";
    // Debian derivatives don't have an unversioned llvm-objcopy command.
    if Command::new(objcopy).output().is_err() {
        objcopy = "llvm-objcopy-13";
    }
    Command::new(objcopy)
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

/// Hashes `bin_file`, then writes the first 4 bytes of the hash into the start
/// of the signature area so that they can be used as a version.
fn write_version_to_stage1_binary(bin_file: &Path) -> Result<()> {
    let mut file_bytes = std::fs::read(bin_file)
        .with_context(|| format!("Failed to open {}", bin_file.display()))?;
    sign_rom::zero_signature(&mut file_bytes)?;
    // Which hash function we use here is irrelevant, so we just use one that we
    // already have a dependency on.
    let mut hasher = hmac_sha256::Hash::new();
    hasher.update(&file_bytes);
    let hash = hasher.finalize();
    file_bytes[mcu_common::SIGNATURE_OFFSET..mcu_common::SIGNATURE_OFFSET + 4]
        .copy_from_slice(&hash[..4]);
    std::fs::write(bin_file, file_bytes)?;
    Ok(())
}

fn sign_stage1_binary(bin_file: &Path) -> Result<()> {
    let mut file_bytes = std::fs::read(bin_file)
        .with_context(|| format!("Failed to open {} for signing", bin_file.display()))?;
    sign_rom::zero_signature(&mut file_bytes)?;
    let hash = sign_rom::hash_image_bytes(&file_bytes)?;
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
