// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use mcu_common::ImageHeader;
use mcu_common::{Hash, Hasher};

extern "C" {
    pub static _slot_start: ImageHeader;
    pub static _slot_end: u32;
}

pub fn load_stage1() -> Result<(), &'static str> {
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

pub fn detect_stage1() -> Result<(), &'static str> {
    let magic = unsafe { _slot_start.magic };
    if magic == 0xC03FEFE {
        defmt::info!("Found stage 1\r");
        return Ok(());
    }
    Err("Could not find stage 1")
}

pub fn stage1_digest() -> Hash {
    let (origin, end): (usize, usize) = get_stage_slot_se();
    const PAGE_SZ: usize = 4 * 256 * 2;
    let mut hasher = Hasher::new();

    // treat the first page with care as we need to adv past the HDR.
    let adv_past = 0x80; // adv past HDR - std::mem::size_of::<ImageHeader>()

    // access page 0 from flash and extend hash..
    let region = unsafe { *((origin + adv_past) as *const [u8; PAGE_SZ]) };
    hasher.add_block(&region); // extend hash a sector at a time.

    //defmt::debug!("{:X} : {:X}", (origin + adv_past), region);
    let start = origin + PAGE_SZ; // step one page forwards.

    for offset in ((0)..(end - start)).step_by(PAGE_SZ) {
        // page flash by 2K steps.
        let region = unsafe { *((start + offset) as *const [u8; PAGE_SZ]) };
        hasher.add_block(&region); // extend hash a sector at a time.

        //defmt::debug!("{:X} : {:X}", (start + offset), region);
    }
    hasher.finalize()
}

fn get_stage_slot_se() -> (usize, usize) {
    let start: usize = unsafe { (&_slot_start as *const _) as usize };
    let end: usize = unsafe { (&_slot_end as *const u32) as usize };
    (start, end)
}
