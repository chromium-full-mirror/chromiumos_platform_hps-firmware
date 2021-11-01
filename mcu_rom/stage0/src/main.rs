// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

mod host_interface;
mod loader;
mod mcu_flash;

use crate::loader::*;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use defmt::debug;
use defmt::error;
use defmt::info;
use defmt::warn;
use defmt_rtt as _; // global logger
use hal::gpio::gpioa;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::SignalEdge;
use hal::prelude::*;
use hal::stm32::I2C1;
use host_interface::HostEvent;
use host_interface::HostInterface;
use mcu_common::Error::McuFlashWriteError;
use mcu_common::MemBlock;
use mcu_flash::flash_error_string;
use mcu_flash::FlashWriter;
use panic_halt as _;
use rtic::app;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

type FwWp = gpioa::PA15<Input<Floating>>;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

enum Stage1Error {
    Missing,
    InvalidSignature,
}

/// A struct that indicates that stage1 is considered valid - either it has
/// passed a signature check or firmware write protect is deasserted.
struct ValidStage1 {
    firmware_version: [u16; 2], // Contains first 4B of validated signature.
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WriteProtectState {
    Asserted,
    Deasserted,
}

/// Whether we're done running stage0 and we should now load stage1.
static BOOTLOAD_TRIGGER: AtomicBool = AtomicBool::new(false);

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        host_interface: HostInterface<Stm32g0I2cPeripheral<I2C1>>,
        flash_writer: FlashWriter,
        fw_wp: FwWp,
        exti: hal::stm32::EXTI,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        static mut I2C_RECV_BUFFER: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE];
        let mut resources = init_peripherals(ctx.device, ctx.core);
        resources
            .host_interface
            .supply_mem_block(MemBlock::new(I2C_RECV_BUFFER));
        resources
    }

    #[task(binds = I2C1, spawn = [handle_event], resources = [host_interface], priority = 2)]
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
                error!("Event queue full!");
            }
        }
    }

    #[task(resources = [flash_writer, fw_wp, host_interface], capacity = 10)]
    fn handle_event(ctx: handle_event::Context, event: HostEvent) {
        let mut host_interface = ctx.resources.host_interface;
        match event {
            HostEvent::LoadApplication => {
                if host_interface.lock(|h| h.stage0_ctx.can_jump_to_stage1()) {
                    BOOTLOAD_TRIGGER.store(true, Ordering::Relaxed);
                }
            }
            HostEvent::WriteFlash(event) => {
                host_interface.lock(|h| h.stage0_ctx.mark_stage1_invalid());
                if let Err(error) = ctx
                    .resources
                    .flash_writer
                    .write(event.address, &*event.data)
                {
                    error!("Failed to write MCU flash: {}", flash_error_string(error));
                    host_interface.lock(|h| h.i2c_report_err(McuFlashWriteError));
                }
                host_interface.lock(|h| h.supply_mem_block(event.data.release()));
                if cfg!(feature = "no-i2c-clock-stretching") {
                    // Reenable I2C. It was disabled above in the I2C interrupt
                    // handler.
                    host_interface.lock(|h| h.i2c_mut().set_i2c_enabled(true));
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

    #[idle(resources = [host_interface])]
    fn idle(mut ctx: idle::Context) -> ! {
        loop {
            // The decision to load stage1 is made in response to an interrupt
            // (e.g. an I2C interrupt) and the decision to execeute stage1 load
            // is made in the idle loop.
            // Loading stage1 from any interrupt handler, would stop CPU from
            // handling any more interrupts at the handler's priority or lower.
            if BOOTLOAD_TRIGGER.load(Ordering::Relaxed) {
                // We disable interrupts (by locking the host_interface) and
                // leave them disabled until stage1 reenables them. Critically,
                // this prevents any requests to write to MCU flash being
                // processed after we've checked that we're OK to jump to
                // stage1.
                ctx.resources.host_interface.lock(|h| {
                    if h.stage0_ctx.can_jump_to_stage1() {
                        load_stage1()
                    }
                });
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
        info!("Button pressed, booting...");
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

    let gpioa = p.GPIOA.split(&mut rcc);
    // NOTE: due to isolation between WP_ODL and the WP signal to HPS,
    // wp is inverted, i.e., de-assert strapped low.
    let fw_wp = gpioa.pa15.into_floating_input();
    let wp_state = if fw_wp.is_low().unwrap() {
        warn!("write-protect de-asserted!");
        WriteProtectState::Deasserted
    } else {
        WriteProtectState::Asserted
    };

    // Configure PB4 as output pin.
    let gpiob = p.GPIOB.split(&mut rcc); // ::gpio::GpioExt
    let mut fpga_pw_gate = gpiob.pb4.into_push_pull_output();

    let gpioc = p.GPIOC.split(&mut rcc); // ::gpio::GpioExt

    // Configure PC15 as output pin.
    let mut fpga_prog = gpioc.pc15.into_push_pull_output();

    let mut exti = p.EXTI;
    gpioc.pc13.listen(SignalEdge::Rising, &mut exti);

    info!("Booting...");

    // The art of implementing a stm32 bootloader in rust.
    info!("{}\n", banner());

    // Put FPGA into reset.
    let _ = fpga_prog.set_low();
    // Power off the FPGA and SPI flash. We don't need either in stage0.
    let _ = fpga_pw_gate.set_high();

    let mut i2c_config = stm32g0_i2c_peripheral::Config::new(
        stm32g0_i2c_peripheral::Address::new_7bit(mcu_common::HPS_ADDRESS),
        stm32g0_i2c_peripheral::Timing::from_speed(&mut rcc, 400.khz()),
    );
    i2c_config.clock_stretching = !cfg!(feature = "no-i2c-clock-stretching");

    let mut host_interface = host_interface::HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb7.into_open_drain_output(),
        gpiob.pb6.into_open_drain_output(),
        i2c_config,
    ));

    host_interface.stage0_ctx.set_write_protect_state(wp_state);

    match detect_and_validate_stage1(wp_state) {
        Ok(valid_stage1) => host_interface.stage0_ctx.set_stage1_valid(valid_stage1),
        Err(e) => {
            error!("Failed to launch application");
            match e {
                Stage1Error::Missing => error!("missing stage 1/app"),
                Stage1Error::InvalidSignature => {
                    error!("invalid stage 1/app signature");
                }
            };
        }
    };

    let flash_writer = FlashWriter::new(p.FLASH, loader::get_stage_slot_se());
    init::LateResources {
        host_interface,
        flash_writer,
        fw_wp,
        exti,
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
