// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

mod debug_led;
mod mcu_interface;

use core::fmt::Write;
pub use debug_led::DebugLed;
use embedded_hal::blocking::spi;
use mcu_interface::McuInterface;

const SOC_ROM_VERSION: u8 = 1;

#[derive(Debug)]
pub enum Error {
    SpiError,
    McuTimeout,
}

type Result<T> = core::result::Result<T, Error>;

pub struct App<S, L>
where
    L: DebugLed,
{
    mcu: McuInterface<S>,
    led: L,
}

impl<S, L> App<S, L>
where
    S: spi::Transfer<u8>,
    L: DebugLed,
{
    pub fn new(spi: S, led: L) -> Self {
        Self {
            mcu: McuInterface::new(spi),
            led,
        }
    }

    pub fn run(&mut self) -> ! {
        self.mcu.resync();
        let _ = self.mcu.report_boot(SOC_ROM_VERSION);
        let _ = writeln!(&mut self.mcu, "Hello from the Rust FPGA");
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
