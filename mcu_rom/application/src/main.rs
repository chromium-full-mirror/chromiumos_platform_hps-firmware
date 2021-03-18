// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use defmt_rtt as _;
use panic_probe as _;

extern crate stm32g0xx_hal as hal;
use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    defmt::info!("Starting MCU application");
    loop {}
}
