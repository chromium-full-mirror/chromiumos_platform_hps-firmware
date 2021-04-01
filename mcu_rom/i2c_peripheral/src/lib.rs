// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

//! This module is a HPS-specific interface to the I2C in peripheral mode. It
//! encodes the following HPS-specific assumptions:
//! - It uses I2C1, not any I2C unit.
//! - It hard-codes the address
//! - Registers have 1 byte addresses
//! - Registers values are 2 bytes and big endian
//!
//! It's also very not generic with the interface it provides.

use core::cell::RefCell;
use cortex_m::interrupt::CriticalSection;
use cortex_m::interrupt::Mutex;
use hal::i2c::Config;
use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::interrupt;
use hal::prelude::*;
use hal::rcc::Rcc;
use hal::stm32;
use stm32g0::stm32g071::I2C1;

const I2C_PERIPHERAL_ADDRESS: u8 = 0x5d;

/// Initializes the I2C interface with the host (I2C1).
pub fn initialize(
    i2c1: I2C1,
    rcc: &mut Rcc,
    sda: impl SDAPin<I2C1>,
    scl: impl SCLPin<I2C1>,
    read_register_callback: &'static ReadRegisterCallback,
    write_register_callback: &'static WriteRegisterCallback,
) {
    let i2c = i2c1.i2c(sda, scl, Config::new(400.khz()), rcc);

    // un-own p.I2C1 block to tap registers
    let (ii2c, _, _) = i2c.release();

    // The I2C unit needs to be disabled while we configure it
    ii2c.cr1.modify(|_, w| w.pe().clear_bit());
    // Enable the kinds of I2C interrupts that we want to handle
    ii2c.cr1.modify(|_, w| w.addrie().set_bit());
    ii2c.cr1.modify(|_, w| w.rxie().set_bit());
    ii2c.cr1.modify(|_, w| w.txie().set_bit());
    // Enable the I2C unit
    ii2c.cr1.modify(|_, w| w.pe().set_bit());

    // Set our I2C address. oa1en need to be clear while we set the address.
    ii2c.oar1.modify(|_, w| w.oa1en().clear_bit());
    ii2c.oar1.modify(|_, w| w.oa1mode().clear_bit()); // 7bit addr
    ii2c.oar1
        .modify(|_, w| unsafe { w.oa1_7_1().bits(I2C_PERIPHERAL_ADDRESS) });
    // Enable automatic ACK of our address.
    ii2c.oar1.modify(|_, w| w.oa1en().set_bit());

    cortex_m::interrupt::free(|cs| {
        I2C_PERIPHERAL
            .borrow(cs)
            .borrow_mut()
            .replace(I2cPeripheral {
                ii2c,
                read_register_callback,
                write_register_callback,
                register: None,
                data: [0u8; 2],
                position: 0,
            });
    });

    // Enable I2C interrupts
    stm32::NVIC::unpend(hal::stm32::Interrupt::I2C1);
    unsafe { stm32::NVIC::unmask(hal::stm32::Interrupt::I2C1) };
}

type ReadRegisterCallback = dyn Sync + Fn(u8, &CriticalSection) -> u16;
type WriteRegisterCallback = dyn Sync + Fn(u8, u16, &CriticalSection);

struct I2cPeripheral {
    ii2c: hal::stm32::I2C1,
    read_register_callback: &'static ReadRegisterCallback,
    write_register_callback: &'static WriteRegisterCallback,

    register: Option<u8>,
    data: [u8; 2],
    position: usize,
}

static I2C_PERIPHERAL: Mutex<RefCell<Option<I2cPeripheral>>> = Mutex::new(RefCell::new(None));

impl I2cPeripheral {
    fn handle_interrupt(&mut self, cs: &CriticalSection) {
        let isr = self.ii2c.isr.read();
        if isr.txis().bit_is_set() {
            let byte = self.next_to_send(cs);
            self.ii2c.txdr.write(|w| unsafe { w.txdata().bits(byte) });
        }
        if isr.rxne().bit_is_set() {
            self.byte_received(self.ii2c.rxdr.read().bits() as u8, cs);
        }
        if isr.addr().bit_is_set() {
            self.ii2c.icr.write(|w| w.addrcf().set_bit());
            if isr.dir().bit_is_set() {
                self.start_read();
            } else {
                self.start_write();
            }
        }
    }

    fn start_read(&mut self) {
        self.position = 2;
        // Setting txe will clear any existing byte already stored into
        // txrd. This is important if the previous read was terminated
        // early by the controller.
        self.ii2c.isr.write(|w| w.txe().set_bit());
    }

    fn start_write(&mut self) {
        self.register = None;
        self.position = 0;
    }

    fn next_to_send(&mut self, cs: &CriticalSection) -> u8 {
        if self.position == self.data.len() {
            if let Some(register) = self.register {
                let value = (self.read_register_callback)(register, cs);
                self.data = [(value >> 8) as u8, (value & 0xff) as u8];
                self.register = Some(register + 1);
            }
            self.position = 0;
        }
        let value = self.data[self.position];
        self.position += 1;
        value
    }

    fn byte_received(&mut self, byte: u8, cs: &CriticalSection) {
        if let Some(register) = self.register {
            self.data[self.position] = byte;
            self.position += 1;
            if self.position == self.data.len() {
                let value = (self.data[0] as u16) << 8 | (self.data[1] as u16);
                (self.write_register_callback)(register, value, cs);
                self.position = 0;
            }
        } else {
            self.register = Some(byte);
        }
    }
}

#[interrupt]
fn I2C1() {
    // If the only places that we access I2C_PERIPHERAL is during initialization
    // and then here in the interrupt handler, we should be able to avoid a
    // critical section. The initialization function writes before it enables
    // interrupts, so there should never be contention. This requires an
    // additional unsafe, so is left for a subsequent CL.
    cortex_m::interrupt::free(|cs| {
        if let Some(peripheral) = I2C_PERIPHERAL.borrow(cs).borrow_mut().as_mut() {
            peripheral.handle_interrupt(cs);
        }
    });
}
