// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use mcu_common::Signature;
use mcu_common::STAGE1_MAGIC;
use mcu_common::{ImageHeader, TargetImageHeader};

extern "C" {
    pub static VECTOR_TABLE: u32;
}

#[link_section = ".image_hdr"]
#[no_mangle]
#[used]
pub static __IMAGE_HDR: TargetImageHeader = ImageHeader {
    magic: STAGE1_MAGIC,
    version: 1,
    vector_addr: unsafe { &VECTOR_TABLE },
    _reserved1: 0,
    _reserved2: 0,
    sig: Signature::empty(), // constructed with padded zeros for unsigned image.
};
