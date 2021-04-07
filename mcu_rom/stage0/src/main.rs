// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]
#![feature(maybe_uninit_ref)]

mod premain;
use crate::premain::*;

mod host_interface;
mod loader;
use crate::loader::*;

// pick a panicking behavior
#[allow(unused)]
// dev profile: easier to debug panics; can put a breakpoint on `rust_begin_unwind`
#[cfg(debug_assertions)]
use panic_halt as _;
// release profile: minimize the binary size of the application (requires nightly?)
#[cfg(not(debug_assertions))]
use panic_abort as _;

use defmt_rtt as _; // global logger

extern crate stm32g0xx_hal as hal;
use cortex_m_rt::entry;

use hal::{prelude::*, stm32};

#[macro_use]
extern crate bitflags;

#[entry]
fn main() -> ! {
    match inner_main() {
        Ok(()) => cortex_m::peripheral::SCB::sys_reset(),
        //Err(e) => panic!(e),
        Err(_) => loop {},
    }
}

fn inner_main() -> Result<(), ()> {
    let p = stm32::Peripherals::take().ok_or(())?;

    // Leave clocks as defaults and let stage 1 configure them.
    let mut rcc = p.RCC.constrain();
    rcc.disable_low_power_mode();

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();
    // configure PB4 as o/p
    let gpiob = p.GPIOB.split(&mut rcc); // ::gpio::GpioExt
    let mut fpga_pw_gate = gpiob.pb4.into_push_pull_output();
    // configure PC14 as i/p
    let gpioc = p.GPIOC.split(&mut rcc); // ::gpio::GpioExt
    let fpga_done = gpioc.pc14.into_floating_input();
    // configure PC15 as o/p
    let mut fpga_prog = gpioc.pc15.into_push_pull_output();

    // Keep the USART pins in a known state early on.
    use hal::serial::FullConfig;
    let _raw_usart = p
        .USART2
        .usart(
            gpioa.pa2, // rx
            gpioa.pa3, // tx
            FullConfig::default().baudrate(115_200.bps()),
            &mut rcc,
        )
        .map_err(drop)?;
    defmt::info!("booting..\r");

    // the art of implementing a stm32 bootloader in rust.
    defmt::info!("{}\n", banner());

    let boot_count = boot_count();
    defmt::info!("boot count = {}\r", boot_count);

    // stop FPGA running and taking SPI flash from us.
    let _ = fpga_prog.set_low(); // SW1 (pressed)?
    let _ = fpga_pw_gate.set_low(); // also gates power to SPI flash!!!
    if fpga_done.is_high().unwrap() {
        defmt::info!("FPGA done\r");
    } else {
        defmt::info!("FPGA wait\r");
    }

    host_interface::service_host_i2c(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
    );

    if let Err(e) = detect_stage1() {
        defmt::info!("{}\r", e);
        return Err(());
    }

    let _ = led.set_high();
    let stage1_digest = stage1_digest().raw_bytes();
    defmt::info!("{:x}\r", stage1_digest);
    let _ = led.set_low();

    load_stage1().map_err(drop)
}

fn banner() -> &'static str {
    r#"
 C
 (\.   \      ,/)
  \(   |\     )/
  //\  | \   /\\
 (/ /\_#oo#_/\ \)
  \/\  ####  /\/
       `##'
quasisec~."#
}
