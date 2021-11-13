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
    let mut app = fpga_app::App::new(
        fpga_hal::McuSpi::new(p.MCU_SPI),
        fpga_hal::DmaImageDataReceiver::new(p.CAM_CONTROL),
        fpga_hal::DebugLed::new(p.LEDS),
    );
    app.run();
}
