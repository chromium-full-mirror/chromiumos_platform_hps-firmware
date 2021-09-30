// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

mod host_interface;
mod loader;
mod mcu_flash;
mod premain;

use crate::loader::*;
use crate::premain::*;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use defmt_rtt as _; // global logger
use hal::gpio::gpioa;
use hal::gpio::SignalEdge;
use hal::gpio::{Floating, PushPull};
use hal::gpio::{Input, Output};
use hal::prelude::*;
use host_interface::HostEvent;
use host_interface::HostInterface;
use host_interface::Stage1OkState;
use mcu_flash::flash_error_string;
use mcu_flash::FlashWriter;
use panic_halt as _;
use rtic::app;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

type Led = gpioa::PA1<Output<PushPull>>;
type FwWp = gpioa::PA15<Input<Floating>>;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

enum Stage1Error {
    Missing,
    InvalidSignature,
}

/// Whether we're done running stage0 and we should now load stage1.
static BOOTLOAD_TRIGGER: AtomicBool = AtomicBool::new(false);

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        host_interface: HostInterface<Stm32g0I2cPeripheral>,
        flash_writer: FlashWriter,
        led: Led,
        fw_wp: FwWp,
        exti: hal::stm32::EXTI,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device, ctx.core)
    }

    #[task(binds = I2C1, spawn = [handle_event], resources = [host_interface])]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        if let Some(event) = ctx.resources.host_interface.handle_i2c_events() {
            if cfg!(feature = "no-i2c-clock-stretching")
                && matches!(event, HostEvent::WriteFlash(_))
            {
                // Disable I2C ACK while writing flash, since writing flash
                // prevents us from responding to I2C events in time.
                // The host can then retry the request until it gets through.
                ctx.resources
                    .host_interface
                    .i2c_mut()
                    .set_i2c_enabled(false);
            }
            if ctx.spawn.handle_event(event).is_err() {
                defmt::error!("Event queue full!");
            }
        }
    }

    #[task(resources = [flash_writer, led, fw_wp, host_interface], capacity = 10)]
    fn handle_event(ctx: handle_event::Context, event: HostEvent) {
        match event {
            HostEvent::LoadApplication => {
                if ctx.resources.host_interface.stage0_ctx.can_stage1_jmp() {
                    BOOTLOAD_TRIGGER.store(true, Ordering::Relaxed);
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
                if cfg!(feature = "no-i2c-clock-stretching") {
                    // Reenable I2C. It was disabled above in the I2C interrupt
                    // handler.
                    ctx.resources.host_interface.i2c_mut().set_i2c_enabled(true);
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

    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        loop {
            // The decision to load stage1 is made in response to an interrupt
            // (e.g. an I2C interrupt) and the decision to execeute stage1 load
            // is made in the idle loop.
            // Loading stage1 from any interrupt handler, would stop CPU from
            // handling any more interrupts at the handler's priority or lower.
            if BOOTLOAD_TRIGGER.load(Ordering::Relaxed) {
                load_stage1();
            }
        }
    }

    #[task(binds = EXTI4_15, resources = [exti], spawn = [handle_event])]
    fn button_click(ctx: button_click::Context) {
        // TODO: Remove this. We don't want this in production.
        ctx.resources.exti.unpend(hal::exti::Event::GPIO13);
        // Don't listen for any further button-presses. If we boot into the
        // application and the button gets pressed, the application won't know
        // to clear the interrupt, so it will keep triggering indefinitely.
        ctx.resources.exti.unlisten(hal::exti::Event::GPIO13);
        defmt::info!("Button pressed, booting...");
        let _ = ctx.spawn.handle_event(HostEvent::LoadApplication);
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
    }
};

fn init_peripherals(
    p: hal::stm32::Peripherals,
    cp: rtic::export::Peripherals,
) -> init::LateResources {
    // Enable DMA clock so that RTT is still handled when we're idle and the CPU
    // is put to sleep.
    p.RCC.ahbenr.modify(|_, w| w.dmaen().set_bit());
    if cfg!(feature = "sys_bootloader") {
        // Reset clock source to HSI. This allows us to be loaded directly from the
        // system bootloader without a reset, since the system bootloader will have
        // already configured the clock source to be the PLL.
        p.RCC.cfgr.write(|w| w);
        // Under reset, our vector table will already have been configured,
        // however if we're loaded from the system bootloader without a reset,
        // it won't be. We have to configure it ourselves.
        unsafe { cp.SCB.vtor.write(&VECTOR_TABLE as *const _ as u32) };
        // The system bootloader leaves the flash in an unlocked state. We
        // expect it to be locked. If we try to unlock it when it's already
        // unlocked then a hard fault will result, so we lock it now. Locking
        // when already locked doesn't cause a fault, so even if the system
        // bootloader sometimes doesn't leave the flash unlocked, this should be
        // safe.
        p.FLASH.cr.modify(|_, w| w.lock().set_bit());
    }
    // Configure clocks so that CPU runs as full speed, otherwise we can't
    // service I2C at 400KHz. Note, we switch this back prior to loading the
    // application.
    let mut rcc = p
        .RCC
        .freeze(hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default()));

    // Configure PA1 as output pin.
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();
    // NOTE: due to isolation between WP_ODL and the WP signal to HPS,
    // wp is inverted, i.e., de-assert strapped low.
    let mut fw_wp = gpioa.pa15.into_floating_input();
    // Configure PB4 as output pin.
    let gpiob = p.GPIOB.split(&mut rcc); // ::gpio::GpioExt
    let mut fpga_pw_gate = gpiob.pb4.into_push_pull_output();
    // Configure PC14 as input pin.
    let gpioc = p.GPIOC.split(&mut rcc); // ::gpio::GpioExt
    let fpga_done = gpioc.pc14.into_floating_input();
    // Configure PC15 as output pin.
    let mut fpga_prog = gpioc.pc15.into_push_pull_output();

    let mut exti = p.EXTI;
    gpioc.pc13.listen(SignalEdge::Rising, &mut exti);

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
    defmt::info!("Booting...\r");

    // The art of implementing a stm32 bootloader in rust.
    defmt::info!("{}\n", banner());

    let boot_count = boot_count();
    defmt::info!("boot count = {}\r", boot_count);

    // Stop FPGA running and taking SPI flash from us.
    let _ = fpga_prog.set_low(); // SW1 (pressed)?
    let _ = fpga_pw_gate.set_low(); // also gates power to SPI flash!!!
    if fpga_done.is_high().unwrap() {
        defmt::info!("FPGA done\r");
    } else {
        defmt::info!("FPGA wait\r");
    }

    let i2c_config = stm32g0_i2c_peripheral::Config {
        clock_stretching: !cfg!(feature = "no-i2c-clock-stretching"),
    };

    let mut host_interface = host_interface::HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb7.into_open_drain_output(),
        gpiob.pb6.into_open_drain_output(),
        mcu_common::HPS_ADDRESS,
        i2c_config,
    ));

    match detect_and_validate_stage1(&mut host_interface, &mut led, &mut fw_wp) {
        Ok(oks) => {
            host_interface.stage0_ctx.set_stage1_present_flag();
            match oks {
                Stage1OkState::ValidSignature => {
                    host_interface.stage0_ctx.set_signature_passed_flag()
                }
                Stage1OkState::InvalidSignatureWpDeassert => {
                    host_interface.stage0_ctx.set_signature_passed_flag()
                }
            }
        }
        Err(e) => {
            defmt::error!("Failed to launch application");
            match e {
                Stage1Error::Missing => defmt::error!("missing stage 1/app"),
                Stage1Error::InvalidSignature => {
                    host_interface.stage0_ctx.set_stage1_present_flag();
                    defmt::error!("invalid stage 1/app signature");
                }
            };
        }
    };

    let flash_writer = FlashWriter::new(p.FLASH, loader::get_stage_slot_se());
    init::LateResources {
        host_interface,
        flash_writer,
        led,
        fw_wp,
        exti,
    }
}

/// Checks whether it's OK to load the application, returning an error if not.
fn detect_and_validate_stage1(
    host_interface: &mut HostInterface<Stm32g0I2cPeripheral>,
    led: &mut Led,
    fw_wp: &mut FwWp,
) -> Result<Stage1OkState, Stage1Error> {
    if let Err(e) = detect_stage1() {
        defmt::info!("{}\r", e);
        return Err(Stage1Error::Missing);
    }

    let _ = led.set_high();
    let stage1_validation = validate_stage1();
    let _ = led.set_low();

    match stage1_validation {
        Ok(version) => {
            defmt::info!("signature good!");
            if fw_wp.is_low().unwrap() {
                defmt::warn!("write-protect de-asserted!");
            }
            host_interface.stage0_ctx.set_firmware_version(version);
            Ok(Stage1OkState::ValidSignature)
        }
        Err(_) => {
            defmt::error!("signature bad!");
            if fw_wp.is_high().unwrap() {
                defmt::warn!("write-protect asserted!");
                defmt::warn!("stage 1 is untrusted, waitting for recovery.");
                Err(Stage1Error::InvalidSignature)
            } else {
                defmt::warn!("write-protect de-asserted!");
                defmt::warn!("starting untrusted stage 1..");
                Ok(Stage1OkState::InvalidSignatureWpDeassert) // unconditionally load in dev mode.
            }
        }
    }
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
