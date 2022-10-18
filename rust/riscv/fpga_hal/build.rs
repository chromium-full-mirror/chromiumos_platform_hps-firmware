// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::PathBuf;

fn main() {
    let out_dir =
        PathBuf::from(std::env::var("OUT_DIR").expect("Environment variable OUT_DIR undefined"));
    let header_path = "../../../build/hps_platform/software/include/generated/soc.h";
    bindgen::Builder::default()
        .header(header_path)
        // If we don't override the target, bindgen will try to generate
        // bindings for riscv, which will fail. All the constants shouldn't
        // depend on our target architecture anyway.
        .clang_arg("--target=x86_64-pc-linux-gnu")
        .generate()
        .expect("Failed to generate bindings for soc.h")
        .write_to_file(out_dir.join("soc.rs"))
        .expect("Failed to write soc.rs");
    println!("cargo:rerun-if-changed={}", header_path);
}
