// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

use i2c_peripheral::I2cError;
use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;
use mcu_common::registers::Register;
use mcu_common::Buffer;
use mcu_common::Error;
use mcu_common::MemBlock;

pub const RECV_BUFFER_SIZE: usize = 256;

#[derive(Debug, Eq, PartialEq)]
pub enum Event {
    None,
    ReadRegister(ReadRegisterEvent),
    WriteRegister(WriteRegisterEvent),
    WriteMemory(WriteMemoryEvent),
}

#[derive(Debug, Eq, PartialEq)]
pub struct ReadRegisterEvent {
    pub register: Register,
}

#[derive(Debug, Eq, PartialEq)]
pub struct WriteRegisterEvent {
    pub register: Register,
    pub value: u16,
}

#[derive(Debug, Eq, PartialEq)]
pub struct WriteMemoryEvent {
    pub bank: u8,
    pub address: u32,
    pub data: Buffer,
}

impl ReadRegisterEvent {
    pub fn respond_u16<I: I2cPeripheral>(self, value: u16, handler: &mut I2cProtocolHandler<I>) {
        handler.write_u16(value);
    }

    /// Sends the data contained in `buffer`. The MemBlock backing the buffer
    /// should have been obtained by calling `take_mem_block` on `handler`.
    pub fn respond<I: I2cPeripheral>(self, buffer: Buffer, handler: &mut I2cProtocolHandler<I>) {
        handler.write_buffer(buffer);
    }
}

/// Handler for an I2C protocol consisting of registers, which can be read and
/// written and memory banks that can be written. This converts low level I2C
/// bus events into higher level register / memory events.
pub struct I2cProtocolHandler<I: I2cPeripheral> {
    i2c: I,
    state: State,
    mem_block: Option<MemBlock>,
}

enum State {
    /// Our initial, default state before we receive any bytes.
    Initial,
    /// In this state, we've received a register number. We don't yet know if
    /// we'll be reading or writing the register.
    AccessingRegister(Register),
    /// We special-case there being a single byte to send, since it allows us to
    /// send 16 bit registers without needing a buffer.
    SendByteQueued(u8),
    // A buffer of bytes to send and a count of how many bytes we've already
    // sent.
    SendBuffer(Buffer, usize),
    WritingRegister(WriteRegisterState),
    /// We enter this state when we have receive a memory bank number and remain
    /// in this state until we've receive the full address.
    MemoryBankAccess(MemoryBankAccessState),
    /// We enter this state once we've receive the address and are receiving
    /// bytes to store into the memory bank.
    MemoryBankWrite(MemoryBankWriteState),
    /// Once we've finished sending or writing the value of a register,
    /// we enter this state and just send zeros, while ignoring everything sent
    /// to us.
    Done,
    /// If we receive some bytes that don't fit the protocol, we enter this state.
    /// We will send zeroes, and ignore everything sent to us.
    /// This also triggers a fault bit if we are sent any more bytes in this state.
    Invalid,
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
    register: Register,
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
            mem_block: None,
        }
    }

    /// Supply (or return) a memory block for the purpose of receiving data.
    pub fn supply_mem_block(&mut self, mem_block: MemBlock) {
        self.mem_block = Some(mem_block);
    }

    /// Returns whether we currently have a buffer into which data can be
    /// received.
    pub fn has_receive_buffer(&self) -> bool {
        self.mem_block.is_some()
    }

    /// Returns the MemBlock which should be filled with data as a Buffer and
    /// returned via `ReadRegisterEvent.respond`.
    pub fn take_mem_block(&mut self) -> Option<MemBlock> {
        self.mem_block.take()
    }

    /// Returns the next event that needs to be handled, consuming that event in
    /// the process.
    pub fn next_event(&mut self) -> Result<Event, Error> {
        loop {
            match self.i2c.next_event() {
                Ok(I2cEvent::StartRead) => {
                    if let State::AccessingRegister(register) = self.state {
                        return Ok(Event::ReadRegister(ReadRegisterEvent { register }));
                    }
                    // We need to send a byte back in response to StartRead. In any unexpected
                    // state, just send zeroes.
                    self.i2c.write_byte(0);
                }
                Ok(I2cEvent::StartWrite) => {
                    self.state = State::Initial;
                }
                Ok(I2cEvent::Stop) => match core::mem::take(&mut self.state) {
                    State::MemoryBankWrite(state) => {
                        return Ok(Event::WriteMemory(WriteMemoryEvent {
                            bank: state.bank,
                            address: state.address,
                            data: state.buffer,
                        }));
                    }
                    State::SendBuffer(buffer, _) => {
                        self.mem_block = Some(buffer.release());
                    }
                    _ => (),
                },
                Ok(I2cEvent::NeedByte) => match &mut self.state {
                    State::SendByteQueued(byte) => self.i2c.write_byte(*byte),
                    State::SendBuffer(buffer, offset) => {
                        if let Some(value) = buffer.get(*offset) {
                            self.i2c.write_byte(*value);
                            *offset += 1;
                        } else {
                            // For now, once we reach the end of the buffer, we
                            // just send zeros.
                            self.i2c.write_byte(0);
                        }
                    }
                    _ => self.i2c.write_byte(0),
                },
                Ok(I2cEvent::ByteReceived(byte)) => match &mut self.state {
                    State::Initial => {
                        // We've received our first byte. Determine if it's a
                        // register access or a memory bank access.
                        if (byte & 0x80) == 0x80 {
                            if let Ok(reg) = (byte & 0x7f).try_into() {
                                self.state = State::AccessingRegister(reg);
                            } else {
                                // Non-existent register
                                self.state = State::Invalid;
                            }
                        } else if (byte & 0xc0) == 0x40 {
                            // Invalid bit pattern
                            self.state = State::Invalid;
                        } else if (byte & 0xc0) == 0x00 {
                            self.state = State::MemoryBankAccess(MemoryBankAccessState {
                                bank: byte & 0x3f,
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
                        return Ok(event);
                    }
                    State::MemoryBankAccess(state) => {
                        // We know which memory bank we're accessing. The byte
                        // is part of the address.
                        state.address_bytes[state.address_bytes_received as usize] = byte;
                        state.address_bytes_received += 1;
                        if state.address_bytes_received as usize == state.address_bytes.len() {
                            if let Some(mem_block) = self.mem_block.take() {
                                self.state = State::MemoryBankWrite(MemoryBankWriteState {
                                    bank: state.bank,
                                    address: u32::from_be_bytes(state.address_bytes),
                                    buffer: Buffer::new(mem_block),
                                });
                            } else {
                                // Attempt to write to memory buffer when the
                                // buffer is already in use. Send all bytes to
                                // /dev/null.
                                self.state = State::Done;
                                return Err(Error::BufferNotAvailable);
                            }
                        }
                    }
                    State::MemoryBankWrite(state) => {
                        if state.buffer.push(byte).is_err() {
                            // Attempt to write beyond end of buffer.
                            self.state = State::Done;
                            return Err(Error::BufferOverrun);
                        }
                    }
                    State::Invalid => {
                        self.state = State::Done;
                        return Err(Error::HostI2cBadRequest);
                    }
                    _ => {}
                },
                Err(nb::Error::WouldBlock) => return Ok(Event::None),
                Err(nb::Error::Other(I2cError::Overrun)) => return Err(Error::HostI2cOverrun),
                Err(nb::Error::Other(I2cError::Underrun)) => return Err(Error::HostI2cOverrun),
                Err(nb::Error::Other(I2cError::BusError)) => return Err(Error::HostI2cBusError),
                _ => {}
            }
        }
    }

    fn write_u16(&mut self, value: u16) {
        let bytes = value.to_be_bytes();
        self.i2c.write_byte(bytes[0]);
        self.state = State::SendByteQueued(bytes[1]);
    }

    fn write_buffer(&mut self, buffer: Buffer) {
        if !buffer.is_empty() {
            self.i2c.write_byte(buffer[0]);
            self.state = State::SendBuffer(buffer, 1);
        } else {
            self.state = State::SendBuffer(buffer, 0);
        }
    }

    pub fn i2c_mut(&mut self) -> &mut I {
        &mut self.i2c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::blocking::i2c::WriteRead;
    use fake_i2c::FakeI2cPeripheral;

    #[test]
    fn test_write_with_invalid_first_byte() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            device
                .write_read(1, &[0x40, 0x00, 0x01, 0x02, 0x03], &mut [])
                .unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert_eq!(protocol_handler.next_event(), Err(Error::HostI2cBadRequest));
        assert_eq!(protocol_handler.next_event(), Ok(Event::None));
    }

    #[test]
    fn test_read_with_invalid_first_byte() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            device.write_read(1, &[0x40], &mut out).unwrap();
            assert_eq!(out, [0, 0])
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert_eq!(protocol_handler.next_event(), Ok(Event::None));

        // Slightly different variation... This represents a more "normal" I2C register access,
        // writing two bytes followed by read.
        let i2c = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            device.write_read(1, &[0x40, 0x00], &mut out).unwrap();
            assert_eq!(out, [0, 0])
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert_eq!(protocol_handler.next_event(), Err(Error::HostI2cBadRequest));
        assert_eq!(protocol_handler.next_event(), Ok(Event::None));
    }

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
            Ok(Event::WriteRegister(WriteRegisterEvent {
                register: Register::SecondPersonStatus,
                value: 0x5ab,
            }))
        );
        assert_eq!(protocol_handler.next_event(), Ok(Event::None));
    }

    #[test]
    fn test_read_register() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            let mut out = [0u8; 2];
            device.write_read(1, &[0x82], &mut out).unwrap();
            assert_eq!(out, [0x12, 0x34])
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        match protocol_handler.next_event() {
            Ok(Event::ReadRegister(event)) => {
                assert_eq!(event.register, Register::SystemStatus);
                event.respond_u16(0x1234, &mut protocol_handler);
            }
            x => panic!("Unexpected event {:?}", x),
        }
        assert_eq!(protocol_handler.next_event(), Ok(Event::None));
    }

    #[test]
    fn test_write_memory_bank() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            device
                .write_read(1, &[1, 0x12, 0x34, 0x56, 0x78, 10, 11, 12], &mut [])
                .unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert!(!protocol_handler.has_receive_buffer());
        protocol_handler.supply_mem_block(MemBlock::with_capacity(64));
        assert!(protocol_handler.has_receive_buffer());
        match protocol_handler.next_event() {
            Ok(Event::WriteMemory(event)) => {
                assert_eq!(event.bank, 1);
                assert_eq!(event.address, 0x12345678);
                assert_eq!(*event.data, [10, 11, 12]);
                assert!(!protocol_handler.has_receive_buffer());
                protocol_handler.supply_mem_block(event.data.release());
                assert!(protocol_handler.has_receive_buffer());
            }
            x => panic!("Unexpected event {:?}", x),
        }
    }

    #[test]
    fn test_write_memory_bank_unavailable() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            device
                .write_read(1, &[1, 0x12, 0x34, 0x56, 0x78, 10, 11, 12], &mut [])
                .unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        assert!(!protocol_handler.has_receive_buffer());
        assert_eq!(
            protocol_handler.next_event(),
            Err(Error::BufferNotAvailable)
        );
        // Keep turning the crank, to let all the bytes be consumed and discarded.
        assert_eq!(protocol_handler.next_event(), Ok(Event::None));
    }

    #[test]
    fn test_write_memory_bank_overrun() {
        let i2c = FakeI2cPeripheral::new(|mut device| {
            device
                .write_read(1, &[1, 0x12, 0x34, 0x56, 0x78, 10, 11, 12], &mut [])
                .unwrap()
        });
        let mut protocol_handler = I2cProtocolHandler::new(i2c);
        protocol_handler.supply_mem_block(MemBlock::with_capacity(2));
        assert_eq!(protocol_handler.next_event(), Err(Error::BufferOverrun));
    }

    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn i2c_buffer_larger_than_crash_record_size() {
        // We use the I2C buffer for storing and sending crash records, so it
        // must large enough to store a crash record.
        assert!(RECV_BUFFER_SIZE >= mcu_common::MCU_CRASH_RECORD_SIZE);
    }
}
