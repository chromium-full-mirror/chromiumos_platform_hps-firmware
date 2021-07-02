// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::ops::Range;

use mcu_common::Hash;
use mcu_common::TargetImageHeader;

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

// TODO press this down into RO fuse memory in the MCU?
const PUBLIC_KEY: [u8; ed25519_compact::PublicKey::BYTES] = [
    0x01, 0xB5, 0x9C, 0x83, 0xD3, 0x8D, 0x40, 0x9B, 0x71, 0x18, 0xA8, 0x38, 0x19, 0xEB, 0x4C, 0x77,
    0x22, 0x01, 0x9D, 0x3C, 0xB7, 0x27, 0x5A, 0x23, 0xC8, 0x45, 0x0C, 0x88, 0xB3, 0xB0, 0x02, 0x6A,
];

extern "C" {
    pub static _hdr: TargetImageHeader;
    pub static _slot_start: u32;
    pub static _slot_end: u32;
}

fn get_stage1_signature() -> [u8; 64] {
    unsafe { _hdr.sig.unwrap() }
}

pub fn load_stage1() -> Result<(), &'static str> {
    // The following is safe provided stage1 sets git_hash_ptr and
    // git_hash_length to refer to valid UTF-8.
    let git_hash = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            _hdr.git_hash_ptr,
            _hdr.git_hash_length as usize,
        ))
    };
    unsafe {
        defmt::info!(" [[stage 1]] >> version={}\r", _hdr.version);
        defmt::debug!(
            " [[stage 1]] >> should I jmp to vector={:x}\r",
            (_hdr.vector_addr as *const u32) as u32
        );
        defmt::info!(" [[stage 1]] >> git hash build={}\r", git_hash);
        defmt::info!(" [[stage 1]] >> sig={:x}\n\r", get_stage1_signature());
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

fn stage1_digest() -> [u8; 32] {
    let slot = get_stage_slot_se();
    let mut hasher = Hash::new();

    const FLASH_END: usize = hal::flash::FLASH_END as usize;
    if slot.end > FLASH_END {
        defmt::debug!("invalid slot.end = {:X}", slot.end);
    }

    // Although we hash the header as well we must treat it a little
    // differently because the signature is a variable position that
    // is obviously inserted after signining. Specifically, we pad out
    // the signature with 0x00 so that it is constant for the purpose
    // of validation.
    let mut region = unsafe { *((slot.start + 0) as *const [u8; PAGE_SIZE]) };
    let sig_offset = unsafe { &_hdr.sig as *const _ as usize - &_hdr as *const _ as usize };
    let pad_value = 0x00;
    let padding: [u8; 64] = [pad_value; 64];
    // clobber signature with padding bytes during verification.
    for idx in 0..64 {
        region[sig_offset + idx] = padding[idx];
    }
    hasher.update(&region);

    for offset in ((PAGE_SIZE)..(slot.end - slot.start)).step_by(PAGE_SIZE) {
        // page flash by 2K steps.
        let region = unsafe { *((slot.start + offset) as *const [u8; PAGE_SIZE]) };
        hasher.update(&region); // extend hash a sector at a time.
    }
    hasher.finalize()
}

pub fn validate_stage1() -> Result<(), ed25519_compact::Error> {
    let stage1_digest = stage1_digest();
    defmt::info!("{:?}", stage1_digest);

    let hdr_sig = get_stage1_signature();
    let sign = ed25519_compact::Signature::new(hdr_sig);
    let vkey = ed25519_compact::PublicKey::new(PUBLIC_KEY);

    vkey.verify(&stage1_digest, &sign)
}

pub(crate) fn get_stage_slot_se() -> Range<usize> {
    let start: usize = unsafe { (&_slot_start as *const _) as usize };
    let end: usize = unsafe { (&_slot_end as *const u32) as usize };
    start..end
}
