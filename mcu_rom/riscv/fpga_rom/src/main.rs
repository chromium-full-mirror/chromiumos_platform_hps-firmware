// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use litex_pac;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let p = litex_pac::Peripherals::take().unwrap();
    // For now we just blink the LED in a distinctive pattern, starting out fast
    // and slowing down, then repeating.
    loop {
        for i in 10..25 {
            for _ in 0..(1 << i) {
                p.LEDS.out.write(|w| w.out().set_bit());
            }
            for _ in 0..(1 << i) {
                p.LEDS.out.write(|w| w.out().clear_bit());
            }
        }
    }
}
