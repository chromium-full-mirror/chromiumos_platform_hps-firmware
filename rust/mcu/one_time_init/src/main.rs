// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use mcu_common::ImageHeader;
use panic_halt as _;
use rtic::app;
use stm32g0 as _;

struct Error;

#[link_section = ".image_hdr"]
#[no_mangle]
#[used]
pub static __IMAGE_HDR: ImageHeader = ImageHeader::empty();

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    #[init]
    fn init(mut ctx: init::Context) {
        // We don't currently have any way to report errors, so we ignore them.
        let _ = configure_reset_pin(&mut ctx.device);
        cortex_m::peripheral::SCB::sys_reset()
    }
};

/// By default, when a software reset is performed, the STM32 pulls the reset
/// pin low and waits for it to actually go low. If something else is pulling it
/// high strongly enough, then we'll never reset. We reconfigure the IRHEN
/// option bit to 0 to change this behavior. It will still pulse the reset pin
/// when a software reset occurs, but it won't wait for it to go low.
fn configure_reset_pin(p: &mut hal::stm32::Peripherals) -> Result<(), Error> {
    use hal::flash::FlashExt;

    let flash = &mut p.FLASH;
    // Unlocking flash results in us losing direct access to the flash
    // registers. We steal the peripherals so that we can get a second
    // handle on the flash, allowing us to unlock it while still retaining
    // access.
    if let Ok(unlocked_flash) = unsafe { hal::stm32::Peripherals::steal() }.FLASH.unlock() {
        flash.optkeyr.write(|w| unsafe { w.bits(0x08192A3B) });
        flash.optkeyr.write(|w| unsafe { w.bits(0x4C5D6E7F) });
        if flash.cr.read().optlock().bit_is_set() {
            unlocked_flash.lock();
            return Err(Error);
        }
        flash.optr.modify(|_, w| w.irhen().clear_bit());
        while flash.sr.read().bsy().bit_is_set() {}
        flash.cr.modify(|_, w| w.optstrt().set_bit());
        while flash.sr.read().bsy().bit_is_set() {}
        unlocked_flash.lock();
        flash.cr.modify(|_, w| w.obl_launch().set_bit());
    }
    Ok(())
}
