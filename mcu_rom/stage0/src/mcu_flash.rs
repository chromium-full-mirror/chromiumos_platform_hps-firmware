// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::ops::Range;
use hal::flash;
use hal::flash::FlashExt;
use hal::flash::FlashPage;
use hal::flash::WriteErase;
use hal::stm32::FLASH;

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

pub struct FlashWriter {
    flash: Option<FLASH>,
    slot: Range<usize>,
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
    pub(crate) fn new(flash: FLASH, slot: Range<usize>) -> FlashWriter {
        FlashWriter {
            flash: Some(flash),
            slot,
        }
    }

    pub(crate) fn write(&mut self, address: u32, data: &[u8]) -> Result<(), flash::Error> {
        // Note, we deliberately shadow to ensure that we don't accidentally use
        // the relative address.
        let address = self.slot.start + address as usize;
        if address + data.len() > self.slot.end {
            return Err(flash::Error::PageOutOfRange);
        }
        if let Some(flash) = self.flash.take() {
            match flash.unlock() {
                Ok(mut unlocked_flash) => {
                    let page = FlashPage((address - flash::FLASH_START) / PAGE_SIZE);
                    // Erase only when we are at the start of our page. This
                    // imposes the constraint on our interface that writes must
                    // always start from the start of a page.
                    if page.to_address() == address {
                        unlocked_flash.erase_page(page)?
                    }
                    unlocked_flash.write(address as usize, data)?;
                    self.flash = Some(unlocked_flash.lock());
                }
                Err(flash) => {
                    self.flash = Some(flash);
                    return Err(flash::Error::Failure);
                }
            }
        }
        defmt::info!(
            "Successfully wrote {} bytes to MCU flash at address {:x}.",
            data.len(),
            address
        );
        Ok(())
    }
}
