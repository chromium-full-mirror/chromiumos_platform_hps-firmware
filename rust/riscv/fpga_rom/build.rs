// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

fn main() {
    let soc_build_dir = Path::new("../../build/hps_platform");
    // Tell linker to look in the directory that contains memory.x
    println!("cargo:rustc-link-search={}", soc_build_dir.display());

    // Putting this in tflm_inference/build.rs doesn't work. Presumably because
    // it affects linking of that library, not linking of the binary.
    if cfg!(feature = "tflm_inference") {
        println!("cargo:rustc-link-arg=-Ttflm_inference.x");
    }
}
