// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use cortex_m_rt::entry;
use defmt_rtt as _;
use hal::interrupt;
use hal::prelude::*;
use hal::stm32;
use host_interface::HostInterface;
use panic_probe as _;
use panic_probe as _;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;
mod host_interface;

static mut I2C_HOST_INTERFACE: Option<HostInterface<Stm32g0I2cPeripheral>> = None;

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

    let host_interface = HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
    ));
    unsafe {
        I2C_HOST_INTERFACE = Some(host_interface);
        I2C_HOST_INTERFACE
            .as_mut()
            .unwrap()
            .i2c()
            .enable_interupts();
    }

    defmt::info!(
        "MCU application started. CPU is running at {}Mhz",
        rcc.clocks.sys_clk.0 / 1000000
    );

    // Loop while we wait for interrupts.
    loop {
        // TODO: Enter some idle state while we're waiting for interrupts.
    }
}

#[interrupt]
fn I2C1() {
    if let Some(interface) = unsafe { &mut I2C_HOST_INTERFACE }.as_mut() {
        interface.handle_interrupt();
    }
}
