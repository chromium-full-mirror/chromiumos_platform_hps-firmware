// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::cell::Cell;
use cortex_m::interrupt::CriticalSection;
use cortex_m::interrupt::Mutex;
use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::rcc::Rcc;
use hal::stm32::I2C1;
use i2c_peripheral::I2cPeripheral;

const HARDWARE_VERSION: u16 = 1;
const RO_VERSION: u16 = 1;

static READY_TO_LAUNCH: Mutex<Cell<bool>> = Mutex::new(Cell::new(false));

pub(crate) fn service_host_i2c(
    i2c1: I2C1,
    rcc: &mut Rcc,
    sda: impl SDAPin<I2C1>,
    scl: impl SCLPin<I2C1>,
) {
    let _host_i2c = I2cPeripheral::new(
        i2c1,
        rcc,
        sda,
        scl,
        &|reg, _cs| match reg {
            0 => mcu_common::hps_magic_code(),
            1 => (HARDWARE_VERSION << 8) | RO_VERSION,
            _ => 0,
        },
        &|reg, value, cs| match reg {
            3 => match value {
                1 => cortex_m::peripheral::SCB::sys_reset(),
                2 => {
                    READY_TO_LAUNCH.borrow(cs).set(true);
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

fn write_memory(bank: u8, address: u32, data: &[u8], _cs: &CriticalSection) {
    // TODO: Implement a bank for writing to the RW portion of the MCU flash.
    defmt::info!("Write memory bank {} @{}: {:?}", bank, address, data);
}
