// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use git_version::git_version;

use mcu_common::ImageHeader;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

#[link_section = ".image_hdr"]
#[no_mangle]
#[used]
pub static __IMAGE_HDR: ImageHeader = ImageHeader {
    magic: 0xC03FEFE,
    version: 1,
    vector_addr: unsafe { &VECTOR_TABLE },
    git_hash_ptr: &build_hash().as_bytes()[0],
    git_hash_length: build_hash().len(),
    sig: [0; 64], // constructed with padded zeros for unsigned image.
};

pub const fn build_hash() -> &'static str {
    git_version!()
}
