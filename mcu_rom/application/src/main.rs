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
    match inner_main() {
        Ok(()) => defmt::info!("inner_main ran successfully"),
        Err(()) => defmt::info!("inner_main got an error"),
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

    host_interface::initialize(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
    );

    Ok(())
}
