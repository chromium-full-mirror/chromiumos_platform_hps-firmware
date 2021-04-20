// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]
#![feature(maybe_uninit_ref)]

mod host_interface;
mod loader;
mod mcu_flash;
mod premain;

use crate::loader::*;
use crate::premain::*;
use host_interface::HostEvent;
use host_interface::HostInterface;
use mcu_flash::flash_error_string;
use mcu_flash::FlashWriter;
// pick a panicking behavior
#[allow(unused)]
// dev profile: easier to debug panics; can put a breakpoint on `rust_begin_unwind`
#[cfg(debug_assertions)]
use panic_halt as _;
// release profile: minimize the binary size of the application (requires nightly?)
use defmt_rtt as _; // global logger
use hal::gpio::gpioa;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::prelude::*;
#[cfg(not(debug_assertions))]
use panic_abort as _;
use rtic::app;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

#[macro_use]
extern crate bitflags;

type Led = gpioa::PA1<Output<PushPull>>;

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        host_interface: HostInterface<Stm32g0I2cPeripheral>,
        flash_writer: FlashWriter,
        led: Led,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    #[task(binds = I2C1, spawn = [handle_event], resources = [host_interface])]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        if let Some(event) = ctx.resources.host_interface.handle_i2c_events() {
            if ctx.spawn.handle_event(event).is_err() {
                defmt::error!("Event queue full!");
            }
        }
    }

    #[task(resources = [flash_writer, led], capacity = 10)]
    fn handle_event(mut ctx: handle_event::Context, event: HostEvent) {
        match event {
            HostEvent::LoadApplication => {
                if load_application(&mut ctx.resources.led).is_err() {
                    defmt::error!("Failed to launch application");
                }
            }
            HostEvent::WriteFlash(event) => {
                if let Err(error) = ctx
                    .resources
                    .flash_writer
                    .write(event.address, &*event.data)
                {
                    defmt::error!("Failed to write MCU flash: {}", flash_error_string(error));
                    // TODO: Report error somewhere
                }
            }
            HostEvent::Reset => {
                // We want to avoid getting a WriteFlash event followed by a
                // Reset event and then processing them out of order, since then
                // the flash wouldn't get written. We achieve this by handling
                // reset here in the same task as WriteFlash, which means they
                // queue and get processed in order.
                cortex_m::peripheral::SCB::sys_reset()
            }
        }
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
    }
};

fn init_peripherals(p: hal::stm32::Peripherals) -> init::LateResources {
    // Enable DMA clock so that RTT is still handled when we're idle and the CPU
    // is put to sleep.
    p.RCC.ahbenr.modify(|_, w| w.dmaen().set_bit());
    // Leave clocks as defaults and let stage 1 configure them.
    let mut rcc = p.RCC.constrain();
    rcc.disable_low_power_mode();

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let led = gpioa.pa1.into_push_pull_output();
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
        .unwrap();
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

    let host_interface = host_interface::HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
    ));
    let flash_writer = FlashWriter::new(p.FLASH, loader::get_stage_slot_se());
    init::LateResources {
        host_interface,
        flash_writer,
        led,
    }
}

fn load_application(led: &mut Led) -> Result<(), ()> {
    if let Err(e) = detect_stage1() {
        defmt::info!("{}\r", e);
        return Err(());
    }

    let _ = led.set_high();
    let stage1_digest = stage1_digest().as_bytes();
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
