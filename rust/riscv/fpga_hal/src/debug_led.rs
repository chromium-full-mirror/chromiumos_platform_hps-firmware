// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub struct DebugLed {
    leds: litex_pac::LEDS,
}

impl DebugLed {
    pub fn new(leds: litex_pac::LEDS) -> Self {
        Self { leds }
    }
}

impl fpga_app::DebugLed for DebugLed {
    fn turn_on(&mut self) {
        self.leds.out.write(|w| w.out().set_bit());
    }

    fn turn_off(&mut self) {
        self.leds.out.write(|w| w.out().clear_bit());
    }
}
