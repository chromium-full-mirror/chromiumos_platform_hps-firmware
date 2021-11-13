// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

mod debug_led;

pub use debug_led::DebugLed;

pub struct App<L>
where
    L: DebugLed,
{
    led: L,
}

impl<L> App<L>
where
    L: DebugLed,
{
    pub fn new(led: L) -> Self {
        Self { led }
    }

    pub fn run(&mut self) -> ! {
        // For now we just blink the LED in a distinctive pattern, starting out fast
        // and slowing down, then repeating.
        loop {
            for i in 10..25 {
                for _ in 0..(1 << i) {
                    self.led.turn_on();
                }
                for _ in 0..(1 << i) {
                    self.led.turn_off();
                }
            }
        }
    }
}
