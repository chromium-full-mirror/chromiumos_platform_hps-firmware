// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::info;
use hal::flash;
use hal::flash::FlashExt;
use hal::flash::FlashPage;
use hal::flash::WriteErase;
use hal::stm32::FLASH;

const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

pub struct FlashWriter {
    flash: Option<FLASH>,
    // Note, the code would still compile if we changed slot here and in
    // `FlashWriter::new` to a shared reference. This is because the write
    // function passes the address as a u32, which the HAL crate then eventually
    // turns into a `*mut u32` which it then writes to. But we do mutate the
    // contents of the slice, so we should hold a mutable reference while we do
    // so.
    slot: &'static mut [u8],
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
    pub(crate) fn new(flash: FLASH, slot: &'static mut [u8]) -> FlashWriter {
        FlashWriter {
            flash: Some(flash),
            slot,
        }
    }

    pub(crate) fn write(&mut self, address: u32, data: &[u8]) -> Result<(), flash::Error> {
        // Note, we deliberately shadow to ensure that we don't accidentally use
        // the relative address.
        let address = &self
            .slot
            .get(address as usize)
            .ok_or(flash::Error::PageOutOfRange)? as *const _ as usize;
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
        info!(
            "Successfully wrote {} bytes to MCU flash at address {:x}.",
            data.len(),
            address
        );
        Ok(())
    }
}
