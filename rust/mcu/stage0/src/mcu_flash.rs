// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hal::flash;
use hal::flash::FlashExt;
use hal::flash::FlashPage;
use hal::flash::UnlockedFlash;
use hal::flash::WriteErase;
use hal::stm32::FLASH;

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

pub struct FlashWriter {
    flash: FlashState,
    // Note, the code would still compile if we changed slot here and in
    // `FlashWriter::new` to a shared reference. This is because the write
    // function passes the address as a u32, which the HAL crate then eventually
    // turns into a `*mut u32` which it then writes to. But we do mutate the
    // contents of the slice, so we should hold a mutable reference while we do
    // so.
    slot: Option<&'static mut [u8]>,
}

enum FlashState {
    Locked(FLASH),
    Unlocked(UnlockedFlash),
    Lost,
}

impl FlashWriter {
    pub(crate) fn new(flash: FLASH, slot: &'static mut [u8]) -> FlashWriter {
        FlashWriter {
            flash: FlashState::Locked(flash),
            slot: Some(slot),
        }
    }

    pub(crate) fn write(&mut self, address: u32, data: &[u8]) -> Result<(), flash::Error> {
        let slot = get_slot(&mut self.slot)?;
        // The hardware will report an error if we try to write beyond the end
        // of flash, but best not to rely on that.
        if address as usize + data.len() > slot.len() {
            return Err(flash::Error::PageOutOfRange);
        }
        // Note, we deliberately shadow to ensure that we don't accidentally use
        // the relative address.
        let address = slot
            .get(address as usize)
            .ok_or(flash::Error::PageOutOfRange)? as *const u8 as usize;
        let unlocked_flash = self.flash.unlocked()?;
        unlocked_flash.write(address as usize, data)?;
        self.flash.lock();
        Ok(())
    }

    pub(crate) fn erase_slot(&mut self) -> Result<(), flash::Error> {
        let unlocked_flash = self.flash.unlocked()?;
        let slot = get_slot(&mut self.slot)?;
        let start = slot.as_mut_ptr() as usize;
        for address in (start..(start + slot.len())).step_by(PAGE_SIZE) {
            let page = FlashPage((address - flash::FLASH_START) / PAGE_SIZE);
            unlocked_flash.erase_page(page)?;
        }
        self.flash.lock();
        Ok(())
    }

    /// Caller takes ownership of the slot. Once called, no futher writes to
    /// flash are permitted.
    pub(crate) fn take_slot(&mut self) -> Result<&'static mut [u8], mcu_common::Error> {
        self.slot.take().ok_or(mcu_common::Error::HostI2cBadRequest)
    }
}

fn get_slot<'a>(slot: &'a mut Option<&'static mut [u8]>) -> Result<&'a mut [u8], flash::Error> {
    if let Some(slot) = slot.as_mut() {
        Ok(*slot)
    } else {
        Err(flash::Error::Illegal)
    }
}

impl FlashState {
    fn unlocked(&mut self) -> Result<&mut UnlockedFlash, flash::Error> {
        if let FlashState::Locked(locked) = core::mem::take(self) {
            *self = match locked.unlock() {
                Ok(unlocked) => FlashState::Unlocked(unlocked),
                Err(locked) => FlashState::Locked(locked),
            }
        }
        if let FlashState::Unlocked(unlocked) = self {
            return Ok(unlocked);
        }
        // This should be unreachable unless there's a bug in our code.
        Err(flash::Error::Failure)
    }

    fn lock(&mut self) {
        *self = match core::mem::take(self) {
            FlashState::Unlocked(unlocked) => FlashState::Locked(unlocked.lock()),
            other => other,
        };
    }
}

impl Default for FlashState {
    fn default() -> Self {
        FlashState::Lost
    }
}
