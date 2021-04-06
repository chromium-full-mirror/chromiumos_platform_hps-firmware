// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hmac_sha256::Hash;

use mcu_common::ImageHeader;

extern "C" {
    pub static _slot_start: ImageHeader;
    pub static _slot_end: u32;
}

pub fn loader() -> Result<(), &'static str> {
    let magic = unsafe { _slot_start.magic };
    if magic == 0xC03FEFE {
        defmt::info!("Found stage 1\r");
        // The following is safe provided stage1 sets git_hash_ptr and
        // git_hash_length to refer to valid UTF-8.
        let git_hash = unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                _slot_start.git_hash_ptr,
                _slot_start.git_hash_length,
            ))
        };
        unsafe {
            defmt::info!(" [[stage 1]] >> version={}\r", _slot_start.version);
            defmt::debug!(
                " [[stage 1]] >> should I jmp to vector={:x}\r",
                (_slot_start.vector_addr as *const u32) as u32
            );
            defmt::info!(" [[stage 1]] >> git hash build={}\r", git_hash);
            defmt::info!(
                " [[stage 1]] >> sig={:x}\n\r",
                _slot_start.sig.raw_bytes()[0]
            );
            cortex_m::asm::bootload(_slot_start.vector_addr);
        }
    }
    Err("Could not find stage 1")
}

pub fn stage1_digest() -> [u8; 32] {
    let start: usize = unsafe { (&_slot_start as *const _) as usize };
    let end: usize = unsafe { (&_slot_end as *const u32) as usize };
    const PAGE_SZ: usize = 4 * 256 * 2;
    let mut hasher = Hash::new();
    for offset in ((0)..(end - start)).step_by(PAGE_SZ) {
        // page flash by 2K steps.
        let region = (start + offset) as *const [u8; PAGE_SZ];
        unsafe { hasher.update(*region) }; // extend hash a sector at a time..
    }
    hasher.finalize()
}
