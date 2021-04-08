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

use hal::i2c::Config;
use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::interrupt;
use hal::prelude::*;
use hal::rcc::Rcc;
use hal::stm32;
use stm32g0::stm32g071::I2C1;

const I2C_PERIPHERAL_ADDRESS: u8 = 0x30;

#[non_exhaustive]
pub struct I2cPeripheral {}

impl I2cPeripheral {
    /// Initializes the I2C interface with the host (I2C1).
    pub fn new(
        i2c1: I2C1,
        rcc: &mut Rcc,
        sda: impl SDAPin<I2C1>,
        scl: impl SCLPin<I2C1>,
        read_register_callback: &'static ReadRegisterCallback,
        write_register_callback: &'static WriteRegisterCallback,
        write_memory_callback: &'static WriteMemoryCallback,
    ) -> I2cPeripheral {
        let i2c = i2c1.i2c(sda, scl, Config::new(400.khz()), rcc);

        // un-own p.I2C1 block to tap registers
        let (ii2c, _, _) = i2c.release();

        // The I2C unit needs to be disabled while we configure it
        ii2c.cr1.modify(|_, w| w.pe().clear_bit());
        // Enable the kinds of I2C interrupts that we want to handle
        ii2c.cr1.modify(|_, w| w.addrie().set_bit());
        ii2c.cr1.modify(|_, w| w.rxie().set_bit());
        ii2c.cr1.modify(|_, w| w.txie().set_bit());
        ii2c.cr1.modify(|_, w| w.stopie().set_bit());
        // Enable the I2C unit
        ii2c.cr1.modify(|_, w| w.pe().set_bit());

        // Set our I2C address. oa1en need to be clear while we set the address.
        ii2c.oar1.modify(|_, w| w.oa1en().clear_bit());
        ii2c.oar1.modify(|_, w| w.oa1mode().clear_bit()); // 7bit addr
        ii2c.oar1
            .modify(|_, w| unsafe { w.oa1_7_1().bits(I2C_PERIPHERAL_ADDRESS) });
        // Enable automatic ACK of our address.
        ii2c.oar1.modify(|_, w| w.oa1en().set_bit());

        unsafe { &mut I2C_PERIPHERAL_STATE }.replace(I2cPeripheralState {
            ii2c,
            read_register_callback,
            write_register_callback,
            write_memory_callback,
            rx_buffer: [0u8; 5],
            rx_offset: 0,
            tx_buffer: [0u8; 2],
            tx_offset: 2,
            memory_buffer: [0u8; 256],
            memory_offset: 0,
        });

        // Enable I2C interrupts
        stm32::NVIC::unpend(hal::stm32::Interrupt::I2C1);
        unsafe { stm32::NVIC::unmask(hal::stm32::Interrupt::I2C1) };

        I2cPeripheral {}
    }
}

impl Drop for I2cPeripheral {
    fn drop(&mut self) {
        stm32::NVIC::unpend(hal::stm32::Interrupt::I2C1);
        stm32::NVIC::mask(hal::stm32::Interrupt::I2C1);
        if let Some(i2c) = unsafe { &mut I2C_PERIPHERAL_STATE }.take() {
            i2c.ii2c.cr1.modify(|_, w| w.pe().clear_bit());
        }
    }
}

type ReadRegisterCallback = dyn Sync + Fn(u8) -> u16;
type WriteRegisterCallback = dyn Sync + Fn(u8, u16);
type WriteMemoryCallback = dyn Sync + Fn(u8, u32, &[u8]);

struct I2cPeripheralState {
    ii2c: hal::stm32::I2C1,
    read_register_callback: &'static ReadRegisterCallback,
    write_register_callback: &'static WriteRegisterCallback,
    write_memory_callback: &'static WriteMemoryCallback,

    rx_buffer: [u8; 5],
    rx_offset: usize,
    tx_buffer: [u8; 2],
    tx_offset: usize,
    memory_buffer: [u8; 256],
    memory_offset: usize,
}

static mut I2C_PERIPHERAL_STATE: Option<I2cPeripheralState> = None;

impl I2cPeripheralState {
    fn handle_interrupt(&mut self) {
        let isr = self.ii2c.isr.read();
        if isr.txis().bit_is_set() {
            let byte = self.next_to_send();
            self.ii2c.txdr.write(|w| unsafe { w.txdata().bits(byte) });
        }
        if isr.rxne().bit_is_set() {
            self.byte_received(self.ii2c.rxdr.read().bits() as u8);
        }
        if isr.addr().bit_is_set() {
            self.ii2c.icr.write(|w| w.addrcf().set_bit());
            if isr.dir().bit_is_set() {
                self.start_read();
            } else {
                self.start_write();
            }
        }
        if isr.stopf().bit_is_set() {
            self.ii2c.icr.write(|w| w.stopcf().set_bit());
            self.stop();
        }
    }

    fn start_read(&mut self) {
        self.tx_offset = self.tx_buffer.len();
        self.rx_offset = 0;
        // Setting txe will clear any existing byte already stored into
        // txrd. This is important if the previous read was terminated
        // early by the controller.
        self.ii2c.isr.write(|w| w.txe().set_bit());
    }

    fn start_write(&mut self) {
        self.rx_offset = 0;
        self.memory_offset = 0;
    }

    fn next_to_send(&mut self) -> u8 {
        if self.tx_offset == self.tx_buffer.len() {
            let register = self.register_number();
            let value;
            if self.is_register_access() {
                value = (self.read_register_callback)(register);
            } else {
                // Reads that aren't register access just return 0.
                value = 0;
            }
            self.tx_buffer[0] = (value >> 8) as u8;
            self.tx_buffer[1] = (value & 0xff) as u8;
            self.set_register_number((register + 1) & 0x7f);
            self.tx_offset = 0;
        }
        let value = self.tx_buffer[self.tx_offset];
        self.tx_offset += 1;
        value
    }

    fn stop(&mut self) {
        if self.rx_offset == 5 {
            let bank = self.rx_buffer[0];
            let address = self.memory_address();
            (self.write_memory_callback)(bank, address, &self.memory_buffer[0..self.memory_offset]);
        }
    }

    fn is_register_access(&self) -> bool {
        (self.rx_buffer[0] & 0x80) != 0
    }

    fn register_number(&self) -> u8 {
        self.rx_buffer[0] & 0x7f
    }

    fn memory_address(&self) -> u32 {
        let mut tmp = [0u8; 4];
        tmp.clone_from_slice(&self.rx_buffer[1..5]);
        u32::from_be_bytes(tmp)
    }

    fn set_register_number(&mut self, register: u8) {
        self.rx_buffer[0] = 0x80 | register;
    }

    fn byte_received(&mut self, byte: u8) {
        if self.rx_offset == 5 {
            if self.memory_offset < self.memory_buffer.len() {
                self.memory_buffer[self.memory_offset] = byte;
                self.memory_offset += 1;
            }
            return;
        }
        self.rx_buffer[self.rx_offset] = byte;
        self.rx_offset += 1;
        if self.is_register_access() {
            if self.rx_offset == 3 {
                let value = (self.rx_buffer[1] as u16) << 8 | (self.rx_buffer[2] as u16);
                (self.write_register_callback)(self.register_number(), value);
                self.rx_offset = 1;
            }
        }
    }
}

#[interrupt]
fn I2C1() {
    // We only access I2C_PERIPHERAL_STATE here and prior to enabling I2C
    // interrupts, so there shouldn't be any possibility of concurrent access.
    if let Some(peripheral) = unsafe { &mut I2C_PERIPHERAL_STATE }.as_mut() {
        peripheral.handle_interrupt();
    }
}
