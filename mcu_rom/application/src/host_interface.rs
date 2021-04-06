// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::cell::RefCell;
use cortex_m::interrupt::CriticalSection;
use cortex_m::interrupt::Mutex;
use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::rcc::Rcc;
use i2c_peripheral::I2cPeripheral;
use panic_probe as _;
use stm32g0::stm32g071::I2C1;

/// Initializes the interface with the host.
pub(crate) fn initialize(
    i2c1: I2C1,
    rcc: &mut Rcc,
    sda: impl SDAPin<I2C1>,
    scl: impl SCLPin<I2C1>,
) -> I2cPeripheral {
    I2cPeripheral::new(i2c1, rcc, sda, scl, &read_register, &write_register)
}

static REGISTER_DATA: Mutex<RefCell<RegisterData>> = Mutex::new(RefCell::new(RegisterData::new()));

struct RegisterData {
    reg9: u16,
}

impl RegisterData {
    const fn new() -> RegisterData {
        RegisterData { reg9: 9 }
    }
}

fn read_register(register: u8, cs: &CriticalSection) -> u16 {
    let register_data = REGISTER_DATA.borrow(cs).borrow();
    match register {
        0 => mcu_common::hps_magic_code(),
        // Register 9 is a RW register that's included for now for testing
        // purposes.
        9 => register_data.reg9,
        _ => 0,
    }
}

fn write_register(register: u8, value: u16, cs: &CriticalSection) {
    let mut register_data = REGISTER_DATA.borrow(cs).borrow_mut();
    defmt::info!("write register {}: {}", register, value);
    match register {
        9 => register_data.reg9 = value,
        _ => (),
    }
}
