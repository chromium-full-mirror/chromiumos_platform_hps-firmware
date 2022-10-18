// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    write_memory_x()?;
    build_one_time_init()?;
    Ok(())
}

fn memory_x_contents() -> String {
    format!(
        r#"MEMORY
{{
    HDR   (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    RAM  (xrw) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
}}

SECTIONS
{{
    .image_hdr : {{
        KEEP (*(.image_hdr))
    }} > HDR
}}

PROGRAM_RAM_TARGET_AREA = 0x{:x};
"#,
        load_address(),
        mcu_common::APPLICATION_VECTOR_TABLE_OFFSET,
        load_address() + mcu_common::APPLICATION_VECTOR_TABLE_OFFSET,
        stage1_max_size(),
        mcu_common::MCU_RAM_START,
        mcu_common::PROGRAM_IN_RAM_OFFSET,
        mcu_common::MCU_RAM_START + mcu_common::PROGRAM_IN_RAM_OFFSET,
    )
}

fn load_address() -> u32 {
    if cfg!(feature = "legacy-stage0") {
        0x08010000
    } else {
        mcu_common::APPLICATION_START_ADDRESS
    }
}

fn stage1_max_size() -> u32 {
    mcu_common::FLASH_END_ADDRESS - load_address()
}

fn write_memory_x() -> Result<()> {
    let out_dir =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| anyhow!("OUT_DIR not set"))?);
    let memory_x_filename = out_dir.join("memory.x");
    let contents = memory_x_contents();
    // We only write memory.x if it doesn't exist or needs changing, otherwise
    // we'll trigger a rebuild even if no files have changed.
    if std::fs::read_to_string(&memory_x_filename).unwrap_or_default() != contents {
        std::fs::write(&memory_x_filename, memory_x_contents())
            .with_context(|| format!("Failed to write {}", memory_x_filename.display()))?;
    }
    // Tell cargo to look for memory.x in our out dir.
    println!("cargo:rustc-link-search={}", out_dir.display());
    // Tell cargo to rebuild if memory.x has changed.
    println!("cargo:rerun-if-changed=memory.x");
    Ok(())
}

fn build_one_time_init() -> Result<()> {
    let out_dir =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| anyhow!("OUT_DIR not set"))?);
    let bin_file = out_dir.join("one_time_init.bin");
    Command::new("cargo")
        .current_dir("../one_time_init")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("CARGO_TARGET_DIR", out_dir.join("nested-build"))
        .arg("build")
        .run()?;
    convert_elf_to_bin(
        out_dir.join("nested-build/thumbv6m-none-eabi/debug/one_time_init"),
        &bin_file,
    )
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
