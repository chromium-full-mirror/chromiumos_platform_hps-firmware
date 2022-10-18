// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    if cfg!(feature = "tflm") {
        build_tflm()?;
    }
    let soc_build_dir = Path::new("../../build/hps_platform");
    // Tell linker to look in the directory that contains memory.x
    println!("cargo:rustc-link-search={}", soc_build_dir.display());

    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    // Also tell linker to search in out_dir, so it finds the linker scripts we generate below.
    println!("cargo:rustc-link-search={}", out_dir.display());

    // Make sure that cargo rebuilds our binary if we change the linker script.
    println!("cargo:rerun-if-changed=fpga_rom.x");
    println!(
        "cargo:rustc-link-search={}",
        std::env::current_dir()?.display()
    );

    if cfg!(feature = "tflm") {
        // For libc.a, libm.a and libstdc++.a.
        println!(
            "cargo:rustc-link-search={}",
            find_builtin_lib("libc.a")?.display()
        );
        // For libgcc.a.
        println!(
            "cargo:rustc-link-search={}",
            find_builtin_lib("libgcc.a")?.display()
        );
        // For libtensorflow-microlite.a.
        println!("cargo:rustc-link-search=../../build/riscv-gcc");
    }

    Ok(())
}

fn find_builtin_lib(name: &str) -> Result<PathBuf> {
    let mut gcc_output = Command::new("../../../scripts/run-tool")
        .arg("riscv64-unknown-elf-gcc")
        .arg("-march=rv32im")
        .arg("-mabi=ilp32")
        .arg(format!("-print-file-name={}", name))
        .output()
        .with_context(|| format!("Failed to learn {} path from GCC", name))?
        .stdout;
    gcc_output.pop(); // discard trailing newline
    let path = PathBuf::from(OsString::from_vec(gcc_output));
    // If GCC doesn't find the library we are looking for, it just returns the (unqualified) name
    // that we asked it for, instead of a full path.
    if path == Path::new(name) {
        bail!("GCC cannot find {}", name);
    }
    Ok(path.parent().unwrap().to_path_buf())
}

fn build_tflm() -> Result<()> {
    const TFLM_DIR: &str = "../../../third_party/tflm";

    // Regenerate ninja config
    if !Command::new("../../../scripts/run-tool")
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
    if !Command::new("../../../scripts/run-tool")
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

    println!("cargo:rerun-if-changed=../../../build/riscv-gcc/libtflite-micro.a");
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
