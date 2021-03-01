// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

#[repr(C, packed(4))]
pub struct ImageHeader {
    pub magic: u32,
    pub version: u16,
    pub vector_addr: &'static u32,
    pub git_hash_ptr: &'static u8,
    pub git_hash_length: usize,
    pub sig: [u8; 64],
}
