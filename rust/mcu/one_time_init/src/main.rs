// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use core::convert::TryFrom;
use cortex_m::peripheral::SCB;
use hal::prelude::*;
use hal::rtc::Rtc;
use hal::stm32;
use hal::stm32::I2C1;
use hal::stm32::PWR;
use hal::stm32::RTC;
use i2c_protocol::Event;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::commands::Command;
use mcu_common::registers::Register;
use mcu_common::CommonHostInterface;
use mcu_common::Error;
use mcu_common::OptionBytesConfigRequest;
use mcu_common::Status;
use panic_halt as _;
use stm32g0 as _;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

pub struct State {
    protocol_handler: I2cProtocolHandler<Stm32g0I2cPeripheral<I2C1>>,
    common_interface: CommonHostInterface,
    flash: stm32::FLASH,
    rtc: Rtc,
    pwr: PWR,
    scb: SCB,
}

#[rtic::app(
    device = hal::stm32,
    peripherals = true,
    dispatchers = [TIM17])]
mod app {
    use super::*;

    #[shared]
    struct SharedResources {}

    #[local]
    struct LocalResources {
        state: State,
    }

    #[task(binds = I2C1, local = [state],
        priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        ctx.local.state.handle_i2c_interrupt();
    }

    #[init]
    fn init(ctx: init::Context) -> (SharedResources, LocalResources, init::Monotonics) {
        let p = ctx.device;

        let flash = p.FLASH;

        // The system bootloader leaves the flash in an unlocked state. We
        // expect it to be locked. If we try to unlock it when it's already
        // unlocked then a hard fault will result, so we lock it now. Locking
        // when already locked doesn't cause a fault, so even if the system
        // bootloader sometimes doesn't leave the flash unlocked, this should be
        // safe.
        flash.cr.modify(|_, w| w.lock().set_bit());

        // Reset clock source to HSI. This is necessary because the bootloader
        // (either stage0 or the STM32 system bootloader) will have left the
        // clock source configured as the PLL.
        p.RCC.cfgr.write(|w| w);

        let mut rcc = p.RCC.freeze(hal::rcc::Config::default());
        let mut rtc = p.RTC.constrain(&mut rcc);
        let mut pwr = p.PWR;
        let mut scb = ctx.core.SCB;

        // For some reason the rest of this program doesn't work if we've been
        // loaded from the system bootloader. We detect if that's the case by
        // checking if the reset vector (address 4) is pointing to somewhere in
        // system memory. If it is, then we reload the option bytes, which will
        // cause us to reset into stage0 (which should have been written
        // already).
        if unsafe { core::ptr::read_volatile(0x4 as *const u32) } & 0x1fff0000 == 0x1fff0000 {
            reset_reloading_option_bytes(&mut rtc, &mut pwr, &mut scb);
        }

        // Configure our vector table. This is necessary when being loaded by a
        // bootloader. Neither stage0 nor the STM32 system bootloader will do
        // this for us.
        unsafe { scb.vtor.write(&VECTOR_TABLE as *const _ as u32) };

        let gpiob = p.GPIOB.split(&mut rcc);

        let sda = gpiob.pb7.into_open_drain_output_in_state(PinState::High);
        let scl = gpiob.pb6.into_open_drain_output_in_state(PinState::High);
        let i2c_config = stm32g0_i2c_peripheral::Config::new(
            stm32g0_i2c_peripheral::Address::new_7bit(mcu_common::HPS_ADDRESS),
            stm32g0_i2c_peripheral::Timing::from_speed(&mut rcc, 400.khz()),
        );

        let protocol_handler = I2cProtocolHandler::new(Stm32g0I2cPeripheral::new(
            p.I2C1, &mut rcc, sda, scl, i2c_config,
        ));

        let local = LocalResources {
            state: State {
                protocol_handler,
                flash,
                common_interface: CommonHostInterface::default(),
                rtc,
                pwr,
                scb,
            },
        };
        (SharedResources {}, local, init::Monotonics())
    }
}

impl State {
    fn handle_i2c_interrupt(&mut self) {
        if let Err(error) = self.handle_i2c_event() {
            self.common_interface.report_error(error);
        }
    }

    fn handle_i2c_event(&mut self) -> Result<(), Error> {
        match self.protocol_handler.next_event()? {
            Event::ReadRegister(event) => {
                let common_value = self
                    .common_interface
                    .read_register(event.register)
                    .unwrap_or(0);
                let result = match event.register {
                    Register::SystemStatus => common_value | Status::ONE_TIME_INIT.bits(),
                    Register::ConfigurationOptionBytes => {
                        self.read_applied_option_byte_settings().bits()
                    }
                    _ => common_value,
                };
                event.respond_u16(result, &mut self.protocol_handler);
            }
            Event::WriteRegister(event) => match event.register {
                Register::ConfigurationOptionBytes => {
                    let request = OptionBytesConfigRequest::from_bits(event.value)
                        .ok_or(Error::HostI2cBadRequest)?;
                    configure_option_bytes(&mut self.flash, request)?;
                    if request.contains(OptionBytesConfigRequest::RELOAD) {
                        reset_reloading_option_bytes(&mut self.rtc, &mut self.pwr, &mut self.scb);
                    }
                }
                Register::Command => match Command::try_from(event.value) {
                    Ok(Command::Reset) => cortex_m::peripheral::SCB::sys_reset(),
                    _ => return Err(Error::HostI2cBadRequest),
                },
                _ => return Err(Error::HostI2cBadRequest),
            },
            Event::None => {}
            Event::WriteMemory(_) => return Err(Error::HostI2cBadRequest),
        }
        Ok(())
    }

    fn read_applied_option_byte_settings(&self) -> OptionBytesConfigRequest {
        let mut settings = OptionBytesConfigRequest::empty();
        let optr = self.flash.optr.read();
        if optr.irhen().bit_is_clear() {
            settings |= OptionBytesConfigRequest::RESET_PIN;
        }
        if optr.n_boot_sel().bit_is_set() {
            settings |= OptionBytesConfigRequest::DISABLE_BOOT0_PIN;
        }
        if optr.rdp().bits() == 0xbb {
            settings |= OptionBytesConfigRequest::RDP1;
        }
        if optr.rdp().bits() == 0xcc {
            settings |= OptionBytesConfigRequest::RDP2;
        }
        let wrp1ar = self.flash.wrp1ar.read();
        if wrp1ar.wrp1a_strt().bits() == 0
            && wrp1ar.wrp1a_end().bits() == (mcu_common::STAGE0_NUM_PAGES - 1) as u8
        {
            settings |= OptionBytesConfigRequest::WRITE_PROTECT;
        }
        settings
    }
}

fn configure_option_bytes(
    flash: &mut hal::stm32::FLASH,
    request: OptionBytesConfigRequest,
) -> Result<(), Error> {
    use hal::flash::FlashExt;

    // If we're erasing stage0 and RDP is not enabled, then just erase the first page.
    if request.contains(OptionBytesConfigRequest::ERASE) && flash.optr.read().rdp().bits() == 0xaa {
        // The HAL has a really inconvenient API for locking the flash, which
        // transfers ownership of the flash, so we conjur ourselves a second
        // instance for it to take ownership of.
        let mut unlocked_flash = unsafe { hal::stm32::Peripherals::steal() }
            .FLASH
            .unlock()
            .map_err(|_| Error::McuFlashWriteError)?;
        use hal::flash::WriteErase;
        unlocked_flash
            .erase_page(hal::flash::FlashPage(0))
            .map_err(|_| Error::McuFlashWriteError)?;
        unlocked_flash.lock();
    }

    // Unlocking flash results in us losing direct access to the flash
    // registers. We steal the peripherals so that we can get a second
    // handle on the flash, allowing us to unlock it while still retaining
    // access.
    let unlocked_flash = unsafe { hal::stm32::Peripherals::steal() }
        .FLASH
        .unlock()
        .map_err(|_| Error::McuFlashWriteError)?;
    flash.optkeyr.write(|w| unsafe { w.bits(0x08192A3B) });
    flash.optkeyr.write(|w| unsafe { w.bits(0x4C5D6E7F) });
    if flash.cr.read().optlock().bit_is_set() {
        unlocked_flash.lock();
        return Err(Error::McuFlashWriteError);
    }

    // We write protect from page 0 to page STAGE0_NUM_PAGES - 1. Note,
    // write protection is inclusive of the end-page.
    const WRITE_PROTECT_BITS: u32 = (mcu_common::STAGE0_NUM_PAGES - 1) << 16;
    if request.contains(OptionBytesConfigRequest::WRITE_PROTECT) {
        unsafe {
            core::ptr::write_volatile(&flash.wrp1ar as *const _ as *mut u32, WRITE_PROTECT_BITS)
        };
    }
    if request.contains(OptionBytesConfigRequest::ERASE) {
        unsafe { core::ptr::write_volatile(&flash.wrp1ar as *const _ as *mut u32, 0x3f) };
        flash.acr.modify(|_, w| w.empty().set_bit());
    }

    flash.optr.modify(|_, w| {
        if request.contains(OptionBytesConfigRequest::RESET_PIN) {
            // By default, when a software reset is performed, the STM32 pulls the
            // reset pin low and waits for it to actually go low. If something else
            // is pulling it high strongly enough, then we'll never reset. We
            // reconfigure the IRHEN option bit to 0 to change this behavior. It
            // will still pulse the reset pin when a software reset occurs, but it
            // won't wait for it to go low.
            w.irhen().clear_bit();
        } else {
            w.irhen().set_bit();
        }

        if request.contains(OptionBytesConfigRequest::DISABLE_BOOT0_PIN) {
            // Disable the boot0 pin. Since the first word of flash is
            // non-empty, this means that we will always boot from flash.
            w.n_boot_sel().set_bit();
            w.n_boot0().set_bit();
        } else {
            w.n_boot_sel().clear_bit();
            w.n_boot0().clear_bit();
        }

        if request.contains(OptionBytesConfigRequest::RDP2) {
            // Set RDP level 2. Once done, this can never be undone.
            unsafe { w.rdp().bits(0xcc) };
        } else if request.contains(OptionBytesConfigRequest::RDP1) {
            // Set RDP level 1.
            unsafe { w.rdp().bits(0xbb) };
        } else if request.contains(OptionBytesConfigRequest::ERASE) {
            // Returning to RDP level 0 triggers a full erase.
            unsafe { w.rdp().bits(0xaa) };
        }

        w
    });

    while flash.sr.read().bsy().bit_is_set() {}
    flash.cr.modify(|_, w| w.optstrt().set_bit());
    while flash.sr.read().bsy().bit_is_set() {}

    unlocked_flash.lock();
    Ok(())
}

/// Resets the MCU, reloading the option bytes in the process. We do this by
/// putting the CPU to sleep, then waking it up using the RTC wakup timing. On
/// exit from the sleep state, the CPU will reload the option bytes.
fn reset_reloading_option_bytes(rtc: &mut Rtc, pwr: &mut PWR, scb: &mut SCB) -> ! {
    // Newer versions of stm32g0xx-hal provide a nice interface for doing this,
    // but we haven't upgraded to those newer versions yet, so we just poke the
    // registers directly.

    const STANDBY_MODE: u8 = 0b011;
    pwr.cr1
        .modify(|_, w| unsafe { w.lpms().bits(STANDBY_MODE) });

    scb.set_sleepdeep();

    write_rtc(rtc, |rtc| {
        // Disable any existing wakeup timer. There shouldn't be one running,
        // but RM0444 says to do this first, since the timer cannot be
        // configured when active.
        rtc.cr.modify(|_, w| w.wute().clear_bit());

        // Wait until writing to WUT is allowed.
        while rtc.icsr.read().wutwf().bit_is_clear() {}

        // The RTC is clocked by the LSI which runs at 32KHz. By default, the
        // wakeup clock (WUCKSEL) is 1/16th of the RTC clock, or 2KHz.
        const WAKE_CLOCK: u32 = 2000;
        const DELAY_MS: u32 = 100;
        rtc.wutr
            .write(|w| unsafe { w.bits(DELAY_MS * WAKE_CLOCK / 1000) });

        rtc.cr.modify(|_, w| {
            // Enable wakeup timer.
            w.wute().set_bit();

            // Enable wakeup interrupt. This is necessary in order to use the
            // wakeup timer to wake from a lower power mode.
            w.wutie().set_bit();
            w
        });
    });

    // Wait for interrupt. This is what will actually put the CPU into standby.
    // When it comes out of standby, it will reset.
    cortex_m::asm::wfi();
    // We should never actually reach here.
    loop {
        cortex_m::asm::nop();
    }
}

/// Unlocks the RTC registers then runs `callback` to give it the oportunity to
/// write to those registers, then relocks them.
fn write_rtc(_rtc: &mut Rtc, callback: impl FnOnce(&mut RTC)) {
    // Safety: We hold an exclusive reference to the Rtc, which owns RTC, we
    // just need direct register access in order to perform operations not yet
    // supported by our HAL.
    let mut rtc = unsafe { hal::stm32::Peripherals::steal() }.RTC;

    // Unlock RTC registers. This is done by writing these two magic values in
    // the right order.
    rtc.wpr.write(|w| unsafe { w.bits(0xca) });
    rtc.wpr.write(|w| unsafe { w.bits(0x53) });

    (callback)(&mut rtc);

    // Relock RTC registers by writing any value that isn't the unlock sequence.
    rtc.wpr.write(|w| unsafe { w.bits(0) });
}
