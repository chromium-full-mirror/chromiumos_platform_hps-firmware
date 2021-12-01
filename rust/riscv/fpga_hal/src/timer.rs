// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use fpga_app::Instant;

#[derive(Default)]
pub struct Timer {}

impl fpga_app::Timer for Timer {
    fn now(&self) -> Instant {
        // Since we can't read the high and low words of the timer atomically,
        // we read repeatedly until we get the same value for the high word
        // before and after reading low. The vast majority of the time, this
        // loop will terminate on the first iteration.
        loop {
            let high = riscv::register::mcycleh::read();
            let low = riscv::register::mcycle::read();
            if riscv::register::mcycleh::read() == high {
                return Instant((high as u64) << 32 | (low as u64));
            }
        }
    }

    fn elapsed_ms(&self, start: Instant) -> u32 {
        let elapsed = self.now().0 - start.0;
        (elapsed / (crate::soc::CONFIG_CLOCK_FREQUENCY as u64 / 1000)) as u32
    }
}
