// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

use i2c_peripheral::I2cError;
use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;

#[derive(Debug, Eq, PartialEq)]
pub enum Event<'a> {
    None,
    ReadRegister(ReadRegisterEvent),
    WriteRegister(WriteRegisterEvent),
    WriteMemory(WriteMemoryEvent<'a>),
    Error(I2cError),
}

#[derive(Debug, Eq, PartialEq)]
pub struct ReadRegisterEvent {
    pub register: u8,
}

#[derive(Debug, Eq, PartialEq)]
pub struct WriteRegisterEvent {
    pub register: u8,
    pub value: u16,
}

#[derive(Debug, Eq, PartialEq)]
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

    /// Returns the next event that needs to be handled, consuming that event in
    /// the process.
    pub fn next_event(&mut self) -> Event {
        loop {
            match self.i2c.next_event() {
                I2cEvent::StartRead => {
                    self.tx_offset = self.tx_buffer.len();
                    self.rx_offset = 0;

                    if self.is_register_access() {
                        return Event::ReadRegister(ReadRegisterEvent {
                            register: self.register_number(),
                        });
                    }
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
                    self.send_next_byte();
                }
                I2cEvent::ByteReceived(byte) => {
                    if self.rx_offset == 5 {
                        if self.memory_offset < self.memory_buffer.len() {
                            self.memory_buffer[self.memory_offset] = byte;
                            self.memory_offset += 1;
                        }
                        continue;
                    }
                    self.rx_buffer[self.rx_offset] = byte;
                    self.rx_offset += 1;
                    if self.is_register_access() {
                        if self.rx_offset == 3 {
                            // TODO: Extract out code for dealing with byte ordering.
                            let value =
                                (self.rx_buffer[1] as u16) << 8 | (self.rx_buffer[2] as u16);
                            self.rx_offset = 1;
                            return Event::WriteRegister(WriteRegisterEvent {
                                register: self.register_number(),
                                value,
                            });
                        }
                    }
                }
                I2cEvent::None => return Event::None,
                I2cEvent::Error(error) => return Event::Error(error),
                _ => {}
            }
        }
    }

    fn write_u16(&mut self, value: u16) {
        // TODO: Extract out code for dealing with byte ordering.
        self.tx_buffer[0] = (value >> 8) as u8;
        self.tx_buffer[1] = (value & 0xff) as u8;
        self.tx_offset = 0;
        self.send_next_byte();
    }

    pub fn i2c_mut(&mut self) -> &mut I {
        &mut self.i2c
    }

    /// Sends the next byte from the buffer. Should only be called when our
    /// transfer buffer is known to already contain data.
    fn send_next_byte(&mut self) {
        let value;
        if self.tx_offset == self.tx_buffer.len() {
            value = 0;
        } else {
            value = self.tx_buffer[self.tx_offset];
            self.tx_offset += 1;
        }
        self.i2c.write_byte(value);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::blocking::i2c::WriteRead;
    use fake_i2c::FakeI2cPeripheral;

    #[test]
    fn test_write_register() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            device.write_read(1, &[0x89, 0x05, 0xab], &mut []).unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert_eq!(
            protocol_handler.next_event(),
            Event::WriteRegister(WriteRegisterEvent {
                register: 0x9,
                value: 0x5ab,
            })
        );
    }

    #[test]
    fn test_read_register() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            device.write_read(1, &[0x89], &mut out).unwrap();
            assert_eq!(out, [0x12, 0x34])
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        match protocol_handler.next_event() {
            Event::ReadRegister(event) => {
                assert_eq!(event.register, 0x9);
                event.respond(0x1234, &mut protocol_handler);
            }
            x => panic!("Unexpected event {:?}", x),
        }
        assert_eq!(protocol_handler.next_event(), Event::None);
    }

    #[test]
    fn test_write_memory_bank() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            device
                .write_read(1, &[1, 0x12, 0x34, 0x56, 0x78, 10, 11, 12], &mut [])
                .unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert_eq!(
            protocol_handler.next_event(),
            Event::WriteMemory(WriteMemoryEvent {
                bank: 1,
                address: 0x12345678,
                data: &[10, 11, 12],
            })
        );
    }
}
