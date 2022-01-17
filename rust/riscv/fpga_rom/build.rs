// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

fn main() -> Result<()> {
    if cfg!(feature = "tflm") {
        build_tflm()?;
    }
    let soc_build_dir = Path::new("../../build/hps_platform");
    // Tell linker to look in the directory that contains memory.x
    println!("cargo:rustc-link-search={}", soc_build_dir.display());

    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    write_file(
        &out_dir.join("linker_discard.x"),
        "
    SECTIONS {
        /DISCARD/ :
        {
            *(.init_array)
            *(.gcc_except_table.*)
            *(.eh_frame*)
            abort(*)
        }
    }",
    );

    // The symbol `end` is used by "sbrk" from libnosys. It's unlikely that sbrk
    // is actually used from any reachable code, but linking fails if we don't
    // define `end`.
    write_file(&out_dir.join("tflm_inference.x"), "PROVIDE( end = . );");

    // For tflm_inference.x.
    println!("cargo:rustc-link-search={}", out_dir.display());
    // For libc.a, libm.a and libstdc++.a.
    println!(
        "cargo:rustc-link-search=../../third_party/installed/riscv64-unknown-elf/lib/rv32im/ilp32"
    );
    // For libgcc.a.
    println!("cargo:rustc-link-search=../../third_party/installed/lib/gcc/riscv64-unknown-elf/10.1.0/rv32im/ilp32");
    // For libtensorflow-microlite.a.
    println!("cargo:rustc-link-search=../../build/riscv-gcc");

    Ok(())
}

fn build_tflm() -> Result<()> {
    const TFLM_DIR: &str = "../../../third_party/tflm";

    // Regenerate ninja config
    if !Command::new("../../../scripts/env-run")
        .arg("gn")
        .arg("gen")
        .arg("../../../build")
        .status()
        .context("Failed to run gn gen build")?
        .success()
    {
        bail!("Failed to run `gn gen build`");
    }
    // Run ninja
    if !Command::new("../../../scripts/env-run")
        .arg("ninja")
        .arg("-C")
        .arg("../../../build")
        .arg("riscv-gcc/libtflite-micro.a")
        .status()
        .context("Failed to build libtflite-micro.a with ninja")?
        .success()
    {
        bail!("Failed to build libtflite-micro.a with ninja");
    }

    // Make sure that build.rs gets rerun if any source files in TFLM get
    // changed.
    print_sources_files_in_dir(Path::new(TFLM_DIR))?;
    Ok(())
}

fn print_sources_files_in_dir(tflm_gen_dir: &Path) -> Result<()> {
    const SOURCE_EXTENSIONS: &[&str] = &["h", "cc", "c", "gn"];
    for entry in tflm_gen_dir.read_dir()? {
        let path = entry?.path();
        if path
            .extension()
            .and_then(OsStr::to_str)
            .map(|extension| SOURCE_EXTENSIONS.contains(&extension))
            .unwrap_or(false)
        {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        if path.is_dir() {
            print_sources_files_in_dir(&path)?;
        }
    }
    Ok(())
}

fn write_file(path: &std::path::Path, contents: &str) {
    if std::fs::read_to_string(&path).unwrap_or_default() != contents {
        if let Err(error) = std::fs::write(&path, contents) {
            panic!("Failed to write {}: {}", path.display(), error);
        }
    }
}
