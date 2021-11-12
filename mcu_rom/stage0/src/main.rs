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
use hal::gpio::gpioa;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::prelude::*;
use hal::stm32::I2C1;
use host_interface::HostEvent;
use host_interface::HostInterface;
use mcu_common::Error;
use mcu_common::MemBlock;
use mcu_flash::FlashWriter;
use panic_halt as _;
use rtic::app;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

type FwWp = gpioa::PA15<Input<Floating>>;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

// TODO: Press this down into RO fuse memory in the MCU?
const PUBLIC_KEY: [u8; loader::PUBLIC_KEY_LENGTH] = [
    0x01, 0xB5, 0x9C, 0x83, 0xD3, 0x8D, 0x40, 0x9B, 0x71, 0x18, 0xA8, 0x38, 0x19, 0xEB, 0x4C, 0x77,
    0x22, 0x01, 0x9D, 0x3C, 0xB7, 0x27, 0x5A, 0x23, 0xC8, 0x45, 0x0C, 0x88, 0xB3, 0xB0, 0x02, 0x6A,
];

enum Stage1Error {
    Missing,
    InvalidSignature,
}

/// A struct that indicates that stage1 is considered valid - either it has
/// passed a signature check or firmware write protect is deasserted.
struct ValidStage1 {
    firmware_version: [u16; 2], // Contains first 4B of validated signature.
    vector_address: *const u32,
}

unsafe impl Send for ValidStage1 {}

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
                ctx.resources
                    .host_interface
                    .i2c_report_err(Error::BufferOverrun);
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
                if ctx
                    .resources
                    .flash_writer
                    .write(event.address, &*event.data)
                    .is_err()
                {
                    host_interface.lock(|h| h.i2c_report_err(Error::McuFlashWriteError));
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
            HostEvent::ErasePage0 => {
                if ctx.resources.flash_writer.erase_page0().is_ok() {
                    cortex_m::peripheral::SCB::sys_reset()
                } else {
                    host_interface.lock(|h| h.i2c_report_err(Error::McuFlashWriteError));
                }
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
                        if let Some(valid_stage1) = h.stage0_ctx.valid_stage1() {
                            load_stage1(valid_stage1);
                        }
                    }
                });
            }
        }
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

    // Put FPGA into reset.
    let _ = fpga_prog.set_low();
    // Power off the FPGA and SPI flash. We don't need either in stage0.
    let _ = fpga_pw_gate.set_high();

    let mut i2c_config = stm32g0_i2c_peripheral::Config::new(
        stm32g0_i2c_peripheral::Address::new_7bit(mcu_common::HPS_ADDRESS),
        stm32g0_i2c_peripheral::Timing::from_speed(&mut rcc, 400.khz()),
    );
    i2c_config.clock_stretching = !cfg!(feature = "no-i2c-clock-stretching");

    // Safety: This is the only call to this function.
    let slot = unsafe { loader::get_stage1_slot() };

    // Validating stage1 takes around 690ms. We do it before the I2C bus is
    // initialized so that we NACK rather than clock stretching during
    // validation.
    let valid_stage1_result = detect_and_validate_stage1(slot, &PUBLIC_KEY, wp_state);

    // The HAL crate implements SDAPin and SCLPin only for pins that are already
    // in open-drain mode. But we don't want to put the pins into open-drain
    // mode because that will result in them being temporarily pulled low. We
    // lie to the HAL crate by pretending we've put the pins into open-drain
    // mode, when we haven't. This hack can be removed once we've fixed the HAL
    // crate.
    // safety: Nothing depends on the mode of the pin for memory-safety and we
    // drop the real pins to make sure nothing can mistakenly use them
    core::mem::drop(gpiob.pb7);
    core::mem::drop(gpiob.pb6);
    let sda: hal::gpio::gpiob::PB7<Output<OpenDrain>> = unsafe { core::mem::transmute(()) };
    let scl: hal::gpio::gpiob::PB6<Output<OpenDrain>> = unsafe { core::mem::transmute(()) };

    let mut host_interface = host_interface::HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1, &mut rcc, sda, scl, i2c_config,
    ));

    host_interface.stage0_ctx.set_write_protect_state(wp_state);

    if let Ok(valid_stage1) = valid_stage1_result {
        host_interface.stage0_ctx.set_stage1_valid(valid_stage1)
    };

    let flash_writer = FlashWriter::new(p.FLASH, slot);
    init::LateResources {
        host_interface,
        flash_writer,
        fw_wp,
    }
}

impl ValidStage1 {
    pub fn firmware_version(&self) -> &[u16; 2] {
        &self.firmware_version
    }
}
