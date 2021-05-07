// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! This crate provides an interface to the HPS over I2C. It is abstract over
//! the kind of I2C used, so can be used both from tests and various kinds of
//! actual I2C hardware.

use anyhow::bail;
use anyhow::Result;
use embedded_hal::blocking::i2c;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::HPS_ADDRESS;
use std::time::Duration;

pub trait Hps: Send + 'static {
    fn read_register(&mut self, reg: u8) -> Result<u16>;
    fn write_register(&mut self, reg: u8, value: u16) -> Result<()>;
    fn write_memory(&mut self, bank: u8, address: u32, values: &[u8]) -> Result<()>;

    /// Reads the "magic" register which always contains a particular value if
    /// the device is a HPS. Returns an error on communication failure or if the
    /// returned value isn't what's expected.
    fn check_magic(&mut self) -> Result<()> {
        let magic = self.read_register(registers::MAGIC)?;
        let expected = mcu_common::hps_magic_code();
        if magic != expected {
            bail!(
                "HPS not detected (expected {:x}, read {:x})",
                expected,
                magic
            );
        }
        Ok(())
    }

    fn perform_command(&mut self, command: u16) -> Result<()> {
        self.write_register(registers::COMMAND, command)
    }

    fn wait_memory_bank_available(&mut self, memory_bank: u8) -> Result<()> {
        for _ in 0..100 {
            if let Ok(available) = self.read_register(registers::MEMORY_BANK_AVAILABLE) {
                if available & (1 << memory_bank) != 0 {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        bail!(
            "Timeout while waiting for memory bank {} to become available",
            memory_bank
        );
    }

    fn write_firmware(&mut self, bytes: &[u8]) -> Result<()> {
        let mut address = 0;
        for chunk in bytes.chunks(256) {
            self.wait_memory_bank_available(memory_banks::MCU_RW)?;
            self.write_memory(memory_banks::MCU_RW, address, chunk)?;
            address += chunk.len() as u32;
        }
        Ok(())
    }
}

pub struct HpsAdapter<I, E>
where
    I: i2c::WriteRead<Error = E> + i2c::Write<Error = E>,
{
    sink: I,
}

impl<I, E> HpsAdapter<I, E>
where
    I: i2c::WriteRead<Error = E> + i2c::Write<Error = E> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    pub fn new(sink: I) -> Result<Self> {
        let mut hps = Self::without_magic_check(sink);
        hps.check_magic()?;
        Ok(hps)
    }

    pub fn without_magic_check(sink: I) -> Self {
        Self { sink }
    }
}

impl<I, E> Hps for HpsAdapter<I, E>
where
    I: i2c::WriteRead<Error = E> + i2c::Write<Error = E> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    fn read_register(&mut self, reg: u8) -> Result<u16> {
        let mut buffer = [0u8; 2];
        self.sink
            .write_read(HPS_ADDRESS, &[register_access_command(reg)], &mut buffer)?;
        Ok(((buffer[0] as u16) << 8) | (buffer[1] as u16))
    }

    fn write_register(&mut self, reg: u8, value: u16) -> Result<()> {
        let buffer = [
            register_access_command(reg),
            (value >> 8) as u8,
            (value & 0xff) as u8,
        ];
        Ok(self.sink.write(HPS_ADDRESS, &buffer)?)
    }

    fn write_memory(&mut self, bank: u8, address: u32, values: &[u8]) -> Result<()> {
        if bank >= 64 {
            bail!("Invalid memory bank {}", bank);
        }
        let mut buffer = Vec::with_capacity(1 + std::mem::size_of::<u32>() + values.len());
        buffer.push(bank);
        buffer.extend_from_slice(&address.to_be_bytes());
        buffer.extend_from_slice(values);
        Ok(self.sink.write(HPS_ADDRESS, &buffer)?)
    }
}

fn register_access_command(reg: u8) -> u8 {
    reg | 0x80
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake_i2c::FakeI2cPeripheral;
    use i2c_protocol::Event;
    use i2c_protocol::I2cProtocolHandler;

    #[test]
    fn read_register() {
        let mut i2c = I2cProtocolHandler::new(FakeI2cPeripheral::new(|dev| {
            let mut hps = HpsAdapter::without_magic_check(dev);
            assert_eq!(hps.read_register(42).unwrap(), 99);
        }));
        match i2c.next_event() {
            Event::ReadRegister(event) => {
                assert_eq!(event.register, 42);
                event.respond(99, &mut i2c);
            }
            event => panic!("Unexpected event {:?}", event),
        }
        assert_eq!(i2c.next_event(), Event::None);
    }
}
