// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use hal::prelude::*;
use hal::stm32;
use hal::stm32::I2C1;
use i2c_protocol::Event;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::registers;
use mcu_common::CommonHostInterface;
use mcu_common::Error;
use mcu_common::ImageHeader;
use mcu_common::OptionBytesConfigRequest;
use mcu_common::Status;
use panic_halt as _;
use rtic::app;
use stm32g0 as _;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;
use stm32g0_utils::IntoOpenDrainOutputHigh;

#[link_section = ".image_hdr"]
#[no_mangle]
#[used]
pub static __IMAGE_HDR: ImageHeader = ImageHeader::empty();

extern "C" {
    pub static VECTOR_TABLE: u32;
}

pub struct State {
    protocol_handler: I2cProtocolHandler<Stm32g0I2cPeripheral<I2C1>>,
    common_interface: CommonHostInterface,
    flash: stm32::FLASH,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum OptionBytesConfig {
    /// By default, when a software reset is performed, the STM32 pulls the reset
    /// pin low and waits for it to actually go low. If something else is pulling
    /// it high strongly enough, then we'll never reset. We reconfigure the IRHEN
    /// option bit to 0 to change this behavior. It will still pulse the reset
    /// pin when a software reset occurs, but it won't wait for it to go low.
    ConfigureResetPin,

    /// Don't change any configuration, just reload the option bytes (will
    /// reset).
    Reload,
}

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        state: State,
    }

    #[task(binds = I2C1, resources = [state],
        priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        ctx.resources.state.handle_i2c_interrupt();
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        let p = ctx.device;

        let mut flash = p.FLASH;

        // The system bootloader leaves the flash in an unlocked state. We
        // expect it to be locked. If we try to unlock it when it's already
        // unlocked then a hard fault will result, so we lock it now. Locking
        // when already locked doesn't cause a fault, so even if the system
        // bootloader sometimes doesn't leave the flash unlocked, this should be
        // safe.
        flash.cr.modify(|_, w| w.lock().set_bit());

        // For some reason the rest of this program doesn't work if we've been
        // loaded from the system bootloader. We detect if that's the case by
        // checking if the reset vector (address 4) is pointing to somewhere in
        // system memory. If it is, then we reload the option bytes, which will
        // cause us to reset into stage0 (which should have been written
        // already).
        if unsafe { core::ptr::read_volatile(0x4 as *const u32) } & 0x1fff0000 == 0x1fff0000 {
            let _ = configure_option_bytes(&mut flash, OptionBytesConfigRequest::RELOAD);
        }

        // Reset clock source to HSI. This is necessary because the bootloader
        // (either stage0 or the STM32 system bootloader) will have left the
        // clock source configured as the PLL.
        p.RCC.cfgr.write(|w| w);
        // Configure our vector table. This is necessary when being loaded by a
        // bootloader. Neither stage0 nor the STM32 system bootloader will do
        // this for us.
        unsafe { ctx.core.SCB.vtor.write(&VECTOR_TABLE as *const _ as u32) };

        let mut rcc = p.RCC.freeze(hal::rcc::Config::default());
        let gpiob = p.GPIOB.split(&mut rcc);

        let sda = gpiob.pb7.into_open_drain_output_high();
        let scl = gpiob.pb6.into_open_drain_output_high();
        let i2c_config = stm32g0_i2c_peripheral::Config::new(
            stm32g0_i2c_peripheral::Address::new_7bit(mcu_common::HPS_ADDRESS),
            stm32g0_i2c_peripheral::Timing::from_speed(&mut rcc, 400.khz()),
        );

        let protocol_handler = I2cProtocolHandler::new(Stm32g0I2cPeripheral::new(
            p.I2C1, &mut rcc, sda, scl, i2c_config,
        ));

        init::LateResources {
            state: State {
                protocol_handler,
                flash,
                common_interface: CommonHostInterface::default(),
            },
        }
    }
};

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
                    registers::SYSTEM_STATUS => common_value | Status::ONE_TIME_INIT.bits(),
                    _ => common_value,
                };
                event.respond_u16(result, &mut self.protocol_handler);
            }
            Event::WriteRegister(event) => match event.register {
                registers::CONFIGURE_OPTION_BYTES => {
                    let request = OptionBytesConfigRequest::from_bits(event.value)
                        .ok_or(Error::HOST_I2C_BAD_REQUEST)?;
                    configure_option_bytes(&mut self.flash, request)?;
                }
                _ => return Err(Error::HOST_I2C_BAD_REQUEST),
            },
            Event::None => {}
            Event::WriteMemory(_) => return Err(Error::HOST_I2C_BAD_REQUEST),
        }
        Ok(())
    }
}

fn configure_option_bytes(
    flash: &mut hal::stm32::FLASH,
    request: OptionBytesConfigRequest,
) -> Result<(), Error> {
    use hal::flash::FlashExt;

    // Unlocking flash results in us losing direct access to the flash
    // registers. We steal the peripherals so that we can get a second
    // handle on the flash, allowing us to unlock it while still retaining
    // access.
    let unlocked_flash = unsafe { hal::stm32::Peripherals::steal() }
        .FLASH
        .unlock()
        .map_err(|_| Error::MCU_FLASH_WRITE_ERROR)?;
    flash.optkeyr.write(|w| unsafe { w.bits(0x08192A3B) });
    flash.optkeyr.write(|w| unsafe { w.bits(0x4C5D6E7F) });
    if flash.cr.read().optlock().bit_is_set() {
        unlocked_flash.lock();
        return Err(Error::MCU_FLASH_WRITE_ERROR);
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
        }
        w
    });

    while flash.sr.read().bsy().bit_is_set() {}
    flash.cr.modify(|_, w| w.optstrt().set_bit());
    while flash.sr.read().bsy().bit_is_set() {}

    if request.contains(OptionBytesConfigRequest::RELOAD) {
        // Apply new option bytes. This will reset the MCU.
        flash.cr.modify(|_, w| w.obl_launch().set_bit());

        // Wait for load to complete.
        while flash.cr.read().obl_launch().bit_is_set() {}

        // OBL_LAUNCH should have reset the MCU, so we shouldn't reach here. If we
        // do, something has gone wrong. Relock the flash and return an error.
        unlocked_flash.lock();
        return Err(Error::MCU_FLASH_WRITE_ERROR);
    }

    unlocked_flash.lock();
    Ok(())
}
