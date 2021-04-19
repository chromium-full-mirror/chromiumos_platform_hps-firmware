// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::ops::Range;
use hal::flash;
use hal::flash::FlashExt;
use hal::flash::FlashPage;
use hal::flash::WriteErase;
use hal::stm32::FLASH;
use i2c_peripheral::I2cPeripheral;
use i2c_protocol::I2cProtocolHandler;
use mcu_common::commands;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::status;

const HARDWARE_VERSION: u16 = 1;
const RO_VERSION: u16 = 1;
const PAGE_SIZE: usize = hal::flash::PAGE_SIZE as usize;

pub struct HostInterface<I: I2cPeripheral> {
    i2c: I2cProtocolHandler<I>,
    pub(crate) done: bool,
    flash_write_state: FlashWriteState,
}

impl<I: I2cPeripheral> HostInterface<I> {
    pub(crate) fn new(i2c: I, flash: FLASH, slot: Range<usize>) -> Self {
        Self {
            i2c: I2cProtocolHandler::new(i2c),
            done: false,
            flash_write_state: FlashWriteState {
                flash: Some(flash),
                slot,
            },
        }
    }

    pub(crate) fn handle_i2c_events(&mut self) {
        match self.i2c.next_event() {
            i2c_protocol::Event::ReadRegister(event) => {
                let result = match event.register {
                    registers::MAGIC => mcu_common::hps_magic_code(),
                    registers::HARDWARE_VERSION => (HARDWARE_VERSION << 8) | RO_VERSION,
                    registers::SYSTEM_STATUS => {
                        // Common system status
                        status::OK
                    }
                    _ => 0,
                };
                event.respond(result, &mut self.i2c);
            }
            i2c_protocol::Event::WriteRegister(event) => match event.register {
                registers::COMMAND => match event.value {
                    commands::RESET => cortex_m::peripheral::SCB::sys_reset(),
                    commands::LAUNCH => {
                        self.done = true;
                    }
                    _ => defmt::info!("Unknown command {}", event.value),
                },
                _ => defmt::info!("Write to unsupported register {}", event.register),
            },
            i2c_protocol::Event::WriteMemory(event) => {
                match event.bank {
                    memory_banks::MCU_RW => {
                        if let Err(error) = self.flash_write_state.write(event.address, event.data)
                        {
                            defmt::error!(
                                "Failed to write MCU flash: {}",
                                flash_error_string(error)
                            );
                            // TODO: Report error somewhere
                        }
                        return;
                    }
                    x => {
                        defmt::warn!("Attempt to write to unsupported memory bank {}", x);
                    }
                }
            }
            i2c_protocol::Event::Error(error) => {
                defmt::error!("I2C error: {}", error.as_str())
            }
            _ => {}
        }
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

struct FlashWriteState {
    flash: Option<FLASH>,
    slot: Range<usize>,
}

impl FlashWriteState {
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
