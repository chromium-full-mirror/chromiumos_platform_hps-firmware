// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::alloc::Layout;

use cortex_m::asm;
use alloc_cortex_m::CortexMHeap;

#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

const HEAP_SIZE: usize = 2048; // in bytes

pub fn init_heap() {
    let start = cortex_m_rt::heap_start() as usize;
    unsafe { ALLOCATOR.init(start, HEAP_SIZE) }
}

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    asm::bkpt();
    loop {}
}
