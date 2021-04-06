// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use cortex_m_rt::entry;
use defmt_rtt as _;
use hal::prelude::*;
use hal::stm32;
use panic_probe as _;
use panic_probe as _;
mod host_interface;

#[entry]
fn main() -> ! {
    if let Err(()) = inner_main() {
        defmt::info!("inner_main got an error");
    }
    loop {}
}

fn rcc_config() -> hal::rcc::Config {
    hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default())
}

fn inner_main() -> Result<(), ()> {
    let p = stm32::Peripherals::take().ok_or(())?;
    let mut rcc = p.RCC.freeze(rcc_config());

    let gpiob = p.GPIOB.split(&mut rcc);

    let _i2c_peripheral = host_interface::initialize(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
    );

    defmt::info!("MCU application started");

    // Loop while we wait for interrupts.
    loop {
        // TODO: Enter some idle state while we're waiting for interrupts.
    }
}
