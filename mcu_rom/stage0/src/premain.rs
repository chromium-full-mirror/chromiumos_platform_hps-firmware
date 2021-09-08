// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use cortex_m_rt::pre_init;

bitflags! {
    struct TransientNVMemFlags: u32 {
        const DFU = 0b00000001;
        const SLOT_A = 0b00000010;
        const SLOT_B = 0b00000100;
    }
}
pub struct TransientNVMem {
    magic: u32,
    count: u8,
    // TODO: Use this field.
    #[allow(unused)]
    flags: TransientNVMemFlags,
}

// The initial value assigned here is just to make the compiler happy.
// It's never actually used, since our crt0 doesn't copy into the uninit section.
// We could use MaybeUnit here, except that MaybeUnit::assume_init_ref isn't
// stable.
#[link_section = ".uninit.BOOT0STATE"]
static mut BOOT0STATE: TransientNVMem = TransientNVMem {
    magic: 0,
    count: 0,
    flags: TransientNVMemFlags::empty(),
};

#[pre_init]
unsafe fn init_boot0_state() {
    // Cold on, previous state unknown!
    let magic = BOOT0STATE.magic;
    if magic != 0xC0FFEE {
        BOOT0STATE.count = 0;
        BOOT0STATE.magic = 0xC0FFEE;
    }
    BOOT0STATE.count = BOOT0STATE.count + 1;
}

pub fn boot_count() -> u8 {
    unsafe { BOOT0STATE.count }
}
