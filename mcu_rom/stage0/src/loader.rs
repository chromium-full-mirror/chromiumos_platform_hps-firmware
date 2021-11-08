// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use libstage0::verification::ValidStage1;

extern "C" {
    static mut _slot: [u8; mcu_common::STAGE1_SLOT_LENGTH];
}

/// Loads stage1. This function never returns.
pub(crate) fn load_stage1(valid_stage1: &ValidStage1) -> ! {
    unsafe {
        cortex_m::asm::bootload(valid_stage1.vector_address);
    }
}

/// # Safety
///
/// Caller must ensure that multiple return values from this function do not
/// exist concurrently. This can be ensured by only calling this function once
/// at program start.
pub(crate) unsafe fn get_stage1_slot() -> &'static mut [u8] {
    &mut _slot
}
