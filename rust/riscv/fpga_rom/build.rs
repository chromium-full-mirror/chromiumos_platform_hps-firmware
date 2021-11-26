// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

fn main() {
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
    println!("cargo:rustc-link-search=../../third_party/tflm_gen");
}

fn write_file(path: &std::path::Path, contents: &str) {
    if std::fs::read_to_string(&path).unwrap_or_default() != contents {
        if let Err(error) = std::fs::write(&path, contents) {
            panic!("Failed to write {}: {}", path.display(), error);
        }
    }
}
