// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::debug;
use crate::error;
use crate::info;
use crate::warn;
use crate::Stage1Error;
use crate::ValidStage1;
use crate::WriteProtectState;
use core::ops::Range;
use mcu_common::Hash;
use mcu_common::TargetImageHeader;

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

// TODO: Press this down into RO fuse memory in the MCU?
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

fn decode_version(hdr_sig: &[u8; 64]) -> [u16; 2] {
    [
        u16::from_be_bytes([hdr_sig[0], hdr_sig[1]]),
        u16::from_be_bytes([hdr_sig[2], hdr_sig[3]]),
    ]
}

/// Checks whether it's OK to load the application, returning an error if not.
pub(crate) fn detect_and_validate_stage1(
    wp_state: WriteProtectState,
) -> Result<ValidStage1, Stage1Error> {
    if let Err(e) = detect_stage1() {
        info!("{}", e);
        return Err(Stage1Error::Missing);
    }

    let stage1_validation = validate_stage1();

    match stage1_validation {
        Ok(_) => {
            info!("signature good!");
            Ok(ValidStage1 {
                firmware_version: get_stage1_version(),
            })
        }
        Err(_) => {
            error!("signature bad!");
            match wp_state {
                WriteProtectState::Asserted => {
                    warn!("write-protect asserted!");
                    warn!("stage 1 is untrusted, waitting for recovery.");
                    Err(Stage1Error::InvalidSignature)
                }
                WriteProtectState::Deasserted => {
                    warn!("starting untrusted stage 1..");
                    Ok(ValidStage1 {
                        firmware_version: get_stage1_version(),
                    })
                }
            }
        }
    }
}

/// Loads stage1. This function never returns.
pub fn load_stage1() -> ! {
    // The following is safe provided stage1 sets git_hash_ptr and
    // git_hash_length to refer to valid UTF-8.
    let git_hash = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            _hdr.git_hash_ptr,
            _hdr.git_hash_length as usize,
        ))
    };
    unsafe {
        info!(" [[stage 1]] >> version={}", _hdr.version);
        debug!(
            " [[stage 1]] >> should I jmp to vector={:x}",
            (_hdr.vector_addr as *const u32) as u32
        );
        info!(" [[stage 1]] >> git hash build={}", git_hash);
        info!(" [[stage 1]] >> sig={:x}\n", get_stage1_signature());

        cortex_m::asm::bootload(_hdr.vector_addr);
    }
}

pub fn detect_stage1() -> Result<(), &'static str> {
    let magic = unsafe { _hdr.magic };
    if magic == 0xC03FEFE {
        info!("Found stage1.");
        return Ok(());
    }
    Err("Could not find stage1")
}

fn stage1_digest() -> [u8; 32] {
    let slot = get_stage_slot_se();
    let mut hasher = Hash::new();

    const FLASH_END: usize = hal::flash::FLASH_END as usize;
    if slot.end > FLASH_END {
        debug!("invalid slot.end = {:X}", slot.end);
    }

    // Although we hash the header as well we must treat it a little
    // differently because the signature is a variable position that
    // is obviously inserted after signing.
    // Replace the signature with zeros, since that's what was in the
    // file when the digest was computed during the signing process.
    let mut region = unsafe { *(slot.start as *const [u8; PAGE_SIZE]) };
    let sig_offset = unsafe { &_hdr.sig as *const _ as usize - &_hdr as *const _ as usize };
    for idx in 0..64 {
        region[sig_offset + idx] = 0x00;
    }
    hasher.update(&region);

    for offset in ((PAGE_SIZE)..(slot.end - slot.start)).step_by(PAGE_SIZE) {
        // Page flash by 2K steps.
        let region = unsafe { *((slot.start + offset) as *const [u8; PAGE_SIZE]) };
        hasher.update(&region); // Extend hash one sector at a time.
    }
    hasher.finalize()
}

pub fn validate_stage1() -> Result<(), ed25519_compact::Error> {
    let stage1_digest = stage1_digest();
    info!("{:?}", stage1_digest);

    let hdr_sig = get_stage1_signature();
    let sign = ed25519_compact::Signature::new(hdr_sig);
    let vkey = ed25519_compact::PublicKey::new(PUBLIC_KEY);

    vkey.verify(&stage1_digest, &sign)?;
    Ok(())
}

fn get_stage1_version() -> [u16; 2] {
    let hdr_sig = get_stage1_signature();
    decode_version(&hdr_sig)
}

pub(crate) fn get_stage_slot_se() -> Range<usize> {
    let start: usize = unsafe { (&_slot_start as *const _) as usize };
    let end: usize = unsafe { (&_slot_end as *const u32) as usize };
    start..end
}
