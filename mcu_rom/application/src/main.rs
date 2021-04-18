// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use defmt_rtt as _;
use hal::prelude::*;
use host_interface::HostInterface;
use panic_probe as _;
use panic_probe as _;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;
mod host_interface;
use rtic::app;

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        host_interface: HostInterface<Stm32g0I2cPeripheral>,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    #[task(binds = I2C1, resources = [host_interface])]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        ctx.resources.host_interface.handle_interrupt();
    }
};

fn rcc_config() -> hal::rcc::Config {
    hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default())
}

fn init_peripherals(p: hal::stm32::Peripherals) -> init::LateResources {
    // Enable DMA clock so that RTT (the mechanism used to send defmt messages)
    // is still handled when we're idle and the CPU is put to sleep.
    p.RCC.ahbenr.modify(|_, w| w.dmaen().set_bit());
    let mut rcc = p.RCC.freeze(rcc_config());

    let gpiob = p.GPIOB.split(&mut rcc);

    let host_interface = HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
    ));

    defmt::info!(
        "MCU application started. CPU is running at {}Mhz",
        rcc.clocks.sys_clk.0 / 1000000
    );
    init::LateResources { host_interface }
}
