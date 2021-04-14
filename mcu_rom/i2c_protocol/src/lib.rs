// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;

pub enum Event<'a> {
    None,
    ReadRegister(ReadRegisterEvent),
    WriteRegister(WriteRegisterEvent),
    WriteMemory(WriteMemoryEvent<'a>),
}

pub struct ReadRegisterEvent {
    pub register: u8,
}

pub struct WriteRegisterEvent {
    pub register: u8,
    pub value: u16,
}

pub struct WriteMemoryEvent<'a> {
    pub bank: u8,
    pub address: u32,
    pub data: &'a [u8],
}

impl ReadRegisterEvent {
    pub fn respond<I: I2cPeripheral>(self, value: u16, handler: &mut I2cProtocolHandler<I>) {
        handler.write_u16(value);
    }
}

/// Handler for an I2C protocol consisting of registers, which can be read and
/// written and memory banks that can be written. This converts low level I2C
/// bus events into higher level register / memory events.
pub struct I2cProtocolHandler<I: I2cPeripheral> {
    i2c: I,
    // TODO: See if using an enum to model our different states would result in
    // cleaner code.
    rx_buffer: [u8; 5],
    rx_offset: usize,
    tx_buffer: [u8; 2],
    tx_offset: usize,
    memory_buffer: [u8; 256],
    memory_offset: usize,
}

impl<I: I2cPeripheral> I2cProtocolHandler<I> {
    pub fn new(i2c: I) -> Self {
        Self {
            i2c,
            rx_buffer: [0u8; 5],
            rx_offset: 0,
            tx_buffer: [0u8; 2],
            tx_offset: 2,
            memory_buffer: [0u8; 256],
            memory_offset: 0,
        }
    }

    /// Returns the next event that needs to be handled, consuming that even in
    /// the process.
    pub fn next_event(&mut self) -> Event {
        match self.i2c.next_event() {
            I2cEvent::StartRead => {
                self.tx_offset = self.tx_buffer.len();
                self.rx_offset = 0;
            }
            I2cEvent::StartWrite => {
                self.rx_offset = 0;
                self.memory_offset = 0;
            }
            I2cEvent::Stop => {
                if self.rx_offset == 5 {
                    let bank = self.rx_buffer[0];
                    let address = self.memory_address();
                    return Event::WriteMemory(WriteMemoryEvent {
                        bank,
                        address,
                        data: &self.memory_buffer[0..self.memory_offset],
                    });
                }
            }
            I2cEvent::NeedByte => {
                if self.tx_offset == self.tx_buffer.len() {
                    if self.is_register_access() {
                        let register = self.register_number();
                        self.set_register_number((register + 1) & 0x7f);
                        return Event::ReadRegister(ReadRegisterEvent { register });
                    } else {
                        // Reads that aren't register access just return 0.
                        self.i2c.write_byte(0);
                        return Event::None;
                    }
                }
                let value = self.tx_buffer[self.tx_offset];
                self.tx_offset += 1;
                self.i2c.write_byte(value);
            }
            I2cEvent::ByteReceived(byte) => return self.byte_received(byte),
            _ => {}
        }
        Event::None
    }

    fn write_u16(&mut self, value: u16) {
        // TODO: Extract out code for dealing with byte ordering.
        self.tx_buffer[0] = (value >> 8) as u8;
        self.tx_buffer[1] = (value & 0xff) as u8;
        self.tx_offset = 0;
        // We could consider writing the first byte to the device, or we can
        // just leave it and rely on another NeedsByte event occurring. For now,
        // we do the latter.
    }

    pub fn i2c_mut(&mut self) -> &mut I {
        &mut self.i2c
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

    fn byte_received(&mut self, byte: u8) -> Event {
        if self.rx_offset == 5 {
            if self.memory_offset < self.memory_buffer.len() {
                self.memory_buffer[self.memory_offset] = byte;
                self.memory_offset += 1;
            }
            return Event::None;
        }
        self.rx_buffer[self.rx_offset] = byte;
        self.rx_offset += 1;
        if self.is_register_access() {
            if self.rx_offset == 3 {
                // TODO: Extract out code for dealing with byte ordering.
                let value = (self.rx_buffer[1] as u16) << 8 | (self.rx_buffer[2] as u16);
                self.rx_offset = 1;
                return Event::WriteRegister(WriteRegisterEvent {
                    register: self.register_number(),
                    value,
                });
            }
        }
        Event::None
    }
}
