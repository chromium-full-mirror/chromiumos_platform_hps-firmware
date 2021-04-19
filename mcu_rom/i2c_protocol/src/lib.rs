// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

mod buffer;

pub use buffer::Buffer;

use i2c_peripheral::I2cError;
use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;

#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    None,
    ReadRegister(ReadRegisterEvent),
    WriteRegister(WriteRegisterEvent),
    WriteMemory(WriteMemoryEvent),
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
pub struct WriteMemoryEvent {
    pub bank: u8,
    pub address: u32,
    pub data: Buffer,
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
    state: State,
}

enum State {
    /// Our initial, default state before we receive any bytes.
    Initial,
    /// In this state, we've received a register number. We don't yet know if
    /// we'll be reading or writing the register.
    AccessingRegister(u8),
    /// Registers are two bytes. We send the first byte as soon as we get the
    /// register value, so we only need to store the second byte here.
    SendByteQueued(u8),
    WritingRegister(WriteRegisterState),
    /// We enter this state when we have receive a memory bank number and remain
    /// in this state until we've receive the full address.
    MemoryBankAccess(MemoryBankAccessState),
    /// We enter this state once we've receive the address and are receiving
    /// bytes to store into the memory bank.
    MemoryBankWrite(MemoryBankWriteState),
    /// Once we've finished sending or writing the value of a register, or if we
    /// get some invalid access, such as an attempt to read from a memory bank,
    /// we enter this state and just send zeros, while ignoring everything sent
    /// to us.
    Done,
}

struct MemoryBankAccessState {
    bank: u8,
    address_bytes: [u8; 4],
    address_bytes_received: u8,
}

struct MemoryBankWriteState {
    bank: u8,
    address: u32,
    buffer: Buffer,
}

struct WriteRegisterState {
    register: u8,
    /// The first byte of the value. Once we get the second byte, we're done, so
    /// exit this state.
    first_value_byte: u8,
}

impl Default for State {
    fn default() -> Self {
        State::Initial
    }
}

impl<I: I2cPeripheral> I2cProtocolHandler<I> {
    pub fn new(i2c: I) -> Self {
        Self {
            i2c,
            state: State::Initial,
        }
    }

    /// Returns the next event that needs to be handled, consuming that event in
    /// the process.
    pub fn next_event(&mut self) -> Event {
        loop {
            match self.i2c.next_event() {
                I2cEvent::StartRead => {
                    if let State::AccessingRegister(register) = self.state {
                        return Event::ReadRegister(ReadRegisterEvent { register });
                    }
                }
                I2cEvent::StartWrite => {
                    self.state = State::Initial;
                }
                I2cEvent::Stop => {
                    if let State::MemoryBankWrite(state) = core::mem::take(&mut self.state) {
                        return Event::WriteMemory(WriteMemoryEvent {
                            bank: state.bank,
                            address: state.address,
                            data: state.buffer,
                        });
                    }
                }
                I2cEvent::NeedByte => match self.state {
                    State::SendByteQueued(byte) => self.i2c.write_byte(byte),
                    _ => self.i2c.write_byte(0),
                },
                I2cEvent::ByteReceived(byte) => match &mut self.state {
                    State::Initial => {
                        // We've received our first byte. Determine if it's a
                        // register access or a memory bank access.
                        if let Some(register) = register_from_byte(byte) {
                            self.state = State::AccessingRegister(register);
                        } else {
                            self.state = State::MemoryBankAccess(MemoryBankAccessState {
                                bank: byte,
                                address_bytes: [0u8; 4],
                                address_bytes_received: 0,
                            })
                        }
                    }
                    State::AccessingRegister(register) => {
                        // We've already received a register number and now
                        // there's another byte, that makes this a register
                        // write.
                        self.state = State::WritingRegister(WriteRegisterState {
                            register: *register,
                            first_value_byte: byte,
                        });
                    }
                    State::WritingRegister(state) => {
                        // We already know which register we're writing and have
                        // the first byte. Compute the value and write the
                        // register.
                        let value_bytes = [state.first_value_byte, byte];
                        let event = Event::WriteRegister(WriteRegisterEvent {
                            register: state.register,
                            value: u16::from_be_bytes(value_bytes),
                        });
                        self.state = State::Done;
                        return event;
                    }
                    State::MemoryBankAccess(state) => {
                        // We know which memory bank we're accessing. The byte
                        // is part of the address.
                        state.address_bytes[state.address_bytes_received as usize] = byte;
                        state.address_bytes_received += 1;
                        if state.address_bytes_received as usize == state.address_bytes.len() {
                            if let Some(buffer) = Buffer::new() {
                                self.state = State::MemoryBankWrite(MemoryBankWriteState {
                                    bank: state.bank,
                                    address: u32::from_be_bytes(state.address_bytes),
                                    buffer,
                                });
                            } else {
                                // Attempt to write to memory buffer when the
                                // buffer is already in use. Send all bytes to
                                // /dev/null.
                                // TODO: Report error
                                self.state = State::Done;
                            }
                        }
                    }
                    State::MemoryBankWrite(state) => {
                        if state.buffer.push(byte).is_err() {
                            // Attempt to write beyond end of buffer.
                            // TODO: Report error
                            self.state = State::Done;
                        }
                    }
                    _ => {}
                },
                I2cEvent::None => return Event::None,
                I2cEvent::Error(error) => return Event::Error(error),
                _ => {}
            }
        }
    }

    fn write_u16(&mut self, value: u16) {
        let bytes = value.to_be_bytes();
        self.i2c.write_byte(bytes[0]);
        self.state = State::SendByteQueued(bytes[1]);
    }

    pub fn i2c_mut(&mut self) -> &mut I {
        &mut self.i2c
    }
}

/// Returns the register number represented by `byte` or None if the byte does
/// not represent a register number.
fn register_from_byte(byte: u8) -> Option<u8> {
    if (byte & 0x80) != 0 {
        Some(byte & 0x7f)
    } else {
        None
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
            // The two extra bytes at the end (0x50) are there to make sure we
            // ignore writes beyond the end of a register.
            device
                .write_read(1, &[0x89, 0x05, 0xab, 0x50, 0x50], &mut [])
                .unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert_eq!(
            protocol_handler.next_event(),
            Event::WriteRegister(WriteRegisterEvent {
                register: 0x9,
                value: 0x5ab,
            })
        );
        assert_eq!(protocol_handler.next_event(), Event::None);
    }

    #[test]
    fn test_read_register() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            device.write_read(1, &[0xff], &mut out).unwrap();
            assert_eq!(out, [0x12, 0x34])
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        match protocol_handler.next_event() {
            Event::ReadRegister(event) => {
                assert_eq!(event.register, 0x7f);
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
        match protocol_handler.next_event() {
            Event::WriteMemory(event) => {
                assert_eq!(event.bank, 1);
                assert_eq!(event.address, 0x12345678);
                assert_eq!(*event.data, [10, 11, 12]);
            }
            x => panic!("Unexpected event {:?}", x),
        }
    }
}
