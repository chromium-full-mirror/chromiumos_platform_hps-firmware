// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::ops::Range;

use mcu_common::ImageHeader;
use mcu_common::{Hash, Hasher};

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

extern "C" {
    pub static _hdr: ImageHeader;
    pub static _slot_start: u32;
    pub static _slot_end: u32;
}

pub fn load_stage1() -> Result<(), &'static str> {
    // The following is safe provided stage1 sets git_hash_ptr and
    // git_hash_length to refer to valid UTF-8.
    let git_hash = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            _hdr.git_hash_ptr,
            _hdr.git_hash_length,
        ))
    };
    unsafe {
        defmt::info!(" [[stage 1]] >> version={}\r", _hdr.version);
        defmt::debug!(
            " [[stage 1]] >> should I jmp to vector={:x}\r",
            (_hdr.vector_addr as *const u32) as u32
        );
        defmt::info!(" [[stage 1]] >> git hash build={}\r", git_hash);
        defmt::info!(" [[stage 1]] >> sig={:x}\n\r", _hdr.sig.raw_bytes()[0]);
        cortex_m::asm::bootload(_hdr.vector_addr);
    }
}

pub fn detect_stage1() -> Result<(), &'static str> {
    let magic = unsafe { _hdr.magic };
    if magic == 0xC03FEFE {
        defmt::info!("Found stage 1\r");
        return Ok(());
    }
    Err("Could not find stage 1")
}

pub fn stage1_digest() -> Hash {
    let slot = get_stage_slot_se();
    let mut hasher = Hasher::new();

    for offset in ((0)..(slot.end - slot.start)).step_by(PAGE_SIZE) {
        // page flash by 2K steps.
        let region = unsafe { *((slot.start + offset) as *const [u8; PAGE_SIZE]) };
        hasher.add_block(&region); // extend hash a sector at a time.

        //defmt::debug!("{:X} : {:X}", (start + offset), region);
    }
    hasher.finalize()
}

pub(crate) fn get_stage_slot_se() -> Range<usize> {
    let start: usize = unsafe { (&_slot_start as *const _) as usize };
    let end: usize = unsafe { (&_slot_end as *const u32) as usize };
    start..end
}
