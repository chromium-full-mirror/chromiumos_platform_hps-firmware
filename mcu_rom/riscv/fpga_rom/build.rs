// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

fn main() {
    let soc_build_dir = Path::new("../../build/hps_platform");
    // Tell linker to look in the directory that contains memory.x
    println!("cargo:rustc-link-search={}", soc_build_dir.display());
}
