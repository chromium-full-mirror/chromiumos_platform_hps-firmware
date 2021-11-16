// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub fn stage1_linker_script() -> String {
    format!(
        r#"MEMORY
{{
    HDR   (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}}

SECTIONS
{{
    .image_hdr : {{
        KEEP (*(.image_hdr))
    }} > HDR
}}
"#,
        crate::APPLICATION_START_ADDRESS,
        crate::APPLICATION_VECTOR_TABLE_OFFSET,
        crate::APPLICATION_VECTOR_TABLE_ADDRESS,
        crate::STAGE1_SLOT_LENGTH
    )
}
