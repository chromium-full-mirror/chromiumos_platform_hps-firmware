// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::cell::Cell;
use core::ops::Range;
use cortex_m::interrupt::Mutex;
use hal::flash;
use hal::flash::FlashExt;
use hal::flash::FlashPage;
use hal::flash::WriteErase;
use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::rcc::Rcc;
use hal::stm32::FLASH;
use hal::stm32::I2C1;
use i2c_peripheral::I2cPeripheral;

const HARDWARE_VERSION: u16 = 1;
const RO_VERSION: u16 = 1;
const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

static READY_TO_LAUNCH: Mutex<Cell<bool>> = Mutex::new(Cell::new(false));

static mut FLASH_STATE: FlashWriteState = FlashWriteState::new();

pub(crate) fn service_host_i2c(
    i2c1: I2C1,
    rcc: &mut Rcc,
    sda: impl SDAPin<I2C1>,
    scl: impl SCLPin<I2C1>,
    flash: FLASH,
    slot: Range<usize>,
) {
    // We access FLASH_STATE here during initialization and then again in I2C
    // callbacks. The two cannot happen concurrently because we only enable I2C
    // callbacks after we're done initializing it here.
    let mut state = unsafe { &mut FLASH_STATE };
    state.flash = Some(flash);
    state.slot = slot;
    let _host_i2c = I2cPeripheral::new(
        i2c1,
        rcc,
        sda,
        scl,
        &|reg| match reg {
            0 => mcu_common::hps_magic_code(),
            1 => (HARDWARE_VERSION << 8) | RO_VERSION,
            _ => 0,
        },
        &|reg, value| match reg {
            3 => match value {
                1 => cortex_m::peripheral::SCB::sys_reset(),
                2 => {
                    cortex_m::interrupt::free(|cs| {
                        READY_TO_LAUNCH.borrow(cs).set(true);
                    });
                }
                _ => defmt::info!("Unknown command {}", value),
            },
            _ => defmt::info!("Write to unsupported register {}", reg),
        },
        &write_memory,
    );
    // Wait until we're told to launch, at which point we'll drop the i2c
    // interface and continue execution.
    let mut done = false;
    while !done {
        cortex_m::interrupt::free(|cs| {
            done = READY_TO_LAUNCH.borrow(cs).get();
        });
    }
}

fn flash_error_string(error: flash::Error) -> &'static str {
    match error {
        flash::Error::Busy => "busy",
        flash::Error::Illegal => "illegal",
        flash::Error::EccError => "ecc error",
        flash::Error::PageOutOfRange => "page out of range",
        flash::Error::Failure => "failure",
    }
}

fn write_memory(bank: u8, address: u32, data: &[u8]) {
    match bank {
        0 => {
            if let Err(error) = unsafe { &mut FLASH_STATE }.write(address, data) {
                defmt::error!("Failed to write MCU flash: {}", flash_error_string(error));
                // TODO: Report error somewhere
            }
            return;
        }
        x => {
            defmt::warn!("Attempt to write to unsupported memory bank {}", x);
        }
    }
}

struct FlashWriteState {
    flash: Option<FLASH>,
    slot: Range<usize>,
}

impl FlashWriteState {
    const fn new() -> FlashWriteState {
        FlashWriteState {
            flash: None,
            slot: 0..0,
        }
    }

    fn write(&mut self, address: u32, data: &[u8]) -> Result<(), flash::Error> {
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
            "Successfully wrote {} bytes to MCU flash at address {:x}",
            data.len(),
            address
        );
        Ok(())
    }
}
