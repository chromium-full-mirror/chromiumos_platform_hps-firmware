// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::info;
use core::ops::Range;
use hal::flash;
use hal::flash::FlashExt;
use hal::flash::FlashPage;
use hal::flash::UnlockedFlash;
use hal::flash::WriteErase;
use hal::stm32::FLASH;

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

pub struct FlashWriter {
    flash: FlashState,
    slot: Range<usize>,
}

enum FlashState {
    Locked(FLASH),
    Unlocked(UnlockedFlash),
    Lost,
}

pub(crate) fn flash_error_string(error: flash::Error) -> &'static str {
    match error {
        flash::Error::Busy => "busy",
        flash::Error::Illegal => "illegal",
        flash::Error::EccError => "ecc error",
        flash::Error::PageOutOfRange => "page out of range",
        flash::Error::Failure => "failure",
    }
}

impl FlashWriter {
    // We accept slot as a &'static mut, which means we take ownership of it and
    // the caller can no longer access it. However, we then store it as a range
    // rather than as a slice because if we stored it as a slice, we would be
    // promising to the compiler that nothing else could access that memory
    // other than via the slice, which is a promise we'd be breaking because
    // the HAL writes to that memory on our behalf, but does so via pointers
    // that it creates from integers.
    pub(crate) fn new(flash: FLASH, slot: &'static mut [u8]) -> FlashWriter {
        FlashWriter {
            flash: FlashState::Locked(flash),
            slot: slot.as_ptr() as usize..slot.as_ptr_range().end as usize,
        }
    }

    pub(crate) fn write(&mut self, address: u32, data: &[u8]) -> Result<(), flash::Error> {
        // Note, we deliberately shadow to ensure that we don't accidentally use
        // the relative address.
        let address = self.slot.start + address as usize;
        if address >= self.slot.end {
            return Err(flash::Error::PageOutOfRange);
        }
        let unlocked_flash = self.flash.unlocked()?;
        let page = FlashPage((address - flash::FLASH_START) / PAGE_SIZE);
        // Erase only when we are at the start of our page. This
        // imposes the constraint on our interface that writes must
        // always start from the start of a page.
        if page.to_address() == address {
            unlocked_flash.erase_page(page)?
        }
        unlocked_flash.write(address as usize, data)?;
        self.flash.lock();
        info!(
            "Successfully wrote {} bytes to MCU flash at address {:x}.",
            data.len(),
            address
        );
        Ok(())
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
