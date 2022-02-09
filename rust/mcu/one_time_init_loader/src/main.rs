// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use cortex_m_rt::entry;
use mcu_common::ImageHeader;
use mcu_common::MCU_RAM_SIZE;
use mcu_common::PROGRAM_IN_RAM_OFFSET;
use panic_halt as _;
use stm32g0 as _;

#[link_section = ".image_hdr"]
#[no_mangle]
#[used]
pub static __IMAGE_HDR: ImageHeader = ImageHeader::empty();

const ONE_TIME_INIT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init.bin"));

const TARGET_AREA_SIZE: usize = MCU_RAM_SIZE - PROGRAM_IN_RAM_OFFSET;

extern "C" {
    static mut PROGRAM_RAM_TARGET_AREA: [u8; TARGET_AREA_SIZE];
}

#[entry]
fn main() -> ! {
    let target = unsafe { &mut PROGRAM_RAM_TARGET_AREA };
    target[..ONE_TIME_INIT_BYTES.len()].copy_from_slice(ONE_TIME_INIT_BYTES);
    unsafe {
        cortex_m::asm::bootload(target as *const u8 as *const u32);
    }
}
