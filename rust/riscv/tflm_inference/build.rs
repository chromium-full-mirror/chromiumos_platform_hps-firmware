// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let link_extra = out_dir.join("tflm_inference.x");

    // The symbol `end` is used by "sbrk" from libnosys. It's unlikely that sbrk
    // is actually used from any reachable code, but linking fails if we don't
    // define `end`.
    let link_extra_contents = "
    SECTIONS {
        /DISCARD/ :
        {
            *(.init_array)
            *(.gcc_except_table.*)
            abort(*)
        }
    }

    PROVIDE( end = . );
    ";

    if std::fs::read_to_string(&link_extra).unwrap_or_default() != link_extra_contents {
        if let Err(error) = std::fs::write(&link_extra, link_extra_contents) {
            panic!("Failed to write {}: {}", link_extra.display(), error);
        }
    }
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
