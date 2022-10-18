// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use mcu_common::ImageHeader;

#[cfg_attr(not(feature = "standalone"), link_section = ".image_hdr")]
#[no_mangle]
#[used]
pub static __IMAGE_HDR: ImageHeader = create_image_header();

const fn create_image_header() -> ImageHeader {
    if cfg!(any(
        feature = "dev",
        feature = "image-transfer",
        feature = "no-hash-check"
    )) {
        ImageHeader::tainted()
    } else {
        ImageHeader::empty()
    }
}
