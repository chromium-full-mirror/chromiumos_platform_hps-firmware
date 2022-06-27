// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! This crate provides an interface to the HPS over I2C. It is abstract over
//! the kind of I2C used, so can be used both from tests and various kinds of
//! actual I2C hardware.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::PartIds;
use mcu_common::Status;
use mcu_common::HPS_ADDRESS;
use std::convert::TryFrom;
use std::time::Duration;
use std::time::Instant;

/// How long we'll wait for the HPS to respond on I2C when starting up.
const I2C_START_TIMEOUT: Duration = Duration::from_secs(1);

/// How long we'll wait for the soft CPU to start.
const FPGA_START_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Stage {
    Unknown,
    Stage0,
    Stage1,
    Application,
    OneTimeInit,
}

pub trait Hps {
    fn read_register(&mut self, reg: u8) -> Result<u16>;
    fn read_register_bytes(&mut self, reg: u8, length: usize) -> Result<Vec<u8>>;
    fn write_register(&mut self, reg: u8, value: u16) -> Result<()>;
    fn write_memory(&mut self, bank: u8, address: u32, values: &[u8]) -> Result<()>;

    /// Reads the "magic" register which always contains a particular value if
    /// the device is a HPS. Returns an error on communication failure or if the
    /// returned value isn't what's expected.
    fn check_magic(&mut self) -> Result<()> {
        let magic = self
            .read_register(registers::MAGIC)
            .context("Failed to read magic register")?;
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

    /// Returns the version of stage0 bootloader. Stage0 must be running. The
    /// version returned is the software version - e.g. 3 or 4, not the full
    /// version, e.g. 0x0103.
    fn stage0_version(&mut self) -> Result<u8> {
        let version = self.read_register(registers::HARDWARE_VERSION)?;
        if version == 0 {
            bail!(
                "Failed to read stage0 version. Currently in stage: {:?}",
                self.stage()?
            );
        }
        Ok((version & 0xff) as u8)
    }

    fn status(&mut self) -> Result<Status> {
        Status::from_bits(self.read_register(registers::SYSTEM_STATUS)?)
            .ok_or_else(|| anyhow!("Invalid status bits"))
    }

    fn stage(&mut self) -> Result<Stage> {
        let status = self.status()?;
        if status.contains(Status::STAGE1) {
            Ok(Stage::Stage1)
        } else if status.contains(Status::APPLREADY) {
            Ok(Stage::Application)
        } else if status.contains(Status::STAGE0) || status.contains(Status::DEPRECATED1) {
            Ok(Stage::Stage0)
        } else if status.contains(Status::ONE_TIME_INIT) {
            Ok(Stage::OneTimeInit)
        } else {
            Ok(Stage::Unknown)
        }
    }

    fn firmware_version(&mut self) -> Result<[u8; 4]> {
        let low = self
            .read_register(registers::FIRMWARE_VERSION_LOW)?
            .to_be_bytes();
        let high = self
            .read_register(registers::FIRMWARE_VERSION_HIGH)?
            .to_be_bytes();
        Ok([high[0], high[1], low[0], low[1]])
    }

    fn perform_command(&mut self, command: Command) -> Result<()> {
        self.write_register(registers::COMMAND, command.into())
    }

    fn launch_app(&mut self) -> Result<()> {
        while self.stage()? == Stage::Stage0 {
            self.perform_command(Command::Launch1)?;
        }
        self.perform_command(Command::LaunchApp)?;
        while self.stage()? != Stage::Application {
            self.check_errors()?;
        }
        let start = Instant::now();
        loop {
            if self.read_register(registers::FPGA_BOOT_COUNT)? > 0 {
                break;
            }
            if start.elapsed() > FPGA_START_TIMEOUT {
                bail!(
                    "FPGA soft CPU failed to start within {}s",
                    FPGA_START_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    fn set_feature_enable(&mut self, value: u16) -> Result<()> {
        self.write_register(registers::ENABLED_FEATURES, value)
    }

    fn is_memory_bank_available(&mut self, memory_bank: u8) -> bool {
        if let Ok(available) = self.read_register(registers::MEMORY_BANK_AVAILABLE) {
            if available & (1 << memory_bank) != 0 {
                return true;
            }
        }
        false
    }

    fn read_part_ids(&mut self) -> Result<PartIds> {
        let bytes =
            self.read_register_bytes(registers::PART_IDS, core::mem::size_of::<PartIds>())?;
        Ok(PartIds::from_bytes(&bytes)?)
    }

    fn crash_report(&mut self) -> Result<String> {
        let mut bytes =
            self.read_register_bytes(registers::PREVIOUS_CRASH, mcu_common::MCU_CRASH_RECORD_SIZE)?;
        if let Some(null_offset) = bytes.iter().position(|b| *b == 0) {
            bytes.truncate(null_offset);
        }
        Ok(String::from_utf8(bytes)?)
    }

    fn wait_memory_bank_available(&mut self, memory_bank: u8) -> Result<()> {
        // Writing the first page of the SPI flash will trigger a mass erase.
        // The data sheet for GD25LQ128D lists the maximum chip erase time as
        // 120 seconds. We allow 50% extra just to be sure.
        const TIMEOUT: Duration = Duration::from_secs(180);
        let start = Instant::now();
        loop {
            if self.is_memory_bank_available(memory_bank) {
                return Ok(());
            }
            if start.elapsed() > TIMEOUT {
                bail!(
                    "Timeout while waiting for memory bank {} to become available",
                    memory_bank
                );
            }
        }
    }

    fn write_firmware(&mut self, bytes: &[u8]) -> Result<()> {
        self.perform_command(Command::EraseStage1)?;
        self.wait_memory_bank_available(memory_banks::MCU_RW)?;
        self.check_errors()?;
        let mut address = 0;
        for chunk in bytes.chunks(256) {
            self.write_memory(memory_banks::MCU_RW, address, chunk)?;
            self.wait_memory_bank_available(memory_banks::MCU_RW)?;
            address += chunk.len() as u32;
        }
        self.check_errors()?;
        Ok(())
    }

    fn write_spi_flash(&mut self, bitstream_bytes: &[u8], app_bytes: &[u8]) -> Result<()> {
        self.perform_command(Command::EraseSpiFlash)?;
        self.wait_memory_bank_available(memory_banks::FPGA_BITSTREAM)?;
        self.check_errors()?;
        let mut address = 0;
        for chunk in bitstream_bytes.chunks(256) {
            self.write_memory(memory_banks::FPGA_BITSTREAM, address, chunk)?;
            self.wait_memory_bank_available(memory_banks::FPGA_BITSTREAM)?;
            address += chunk.len() as u32;
        }
        self.check_errors()?;

        address = 0;
        for chunk in app_bytes.chunks(256) {
            self.write_memory(memory_banks::SOC_ROM, address, chunk)?;
            self.wait_memory_bank_available(memory_banks::SOC_ROM)?;
            address += chunk.len() as u32;
        }
        self.check_errors()?;

        Ok(())
    }

    fn error(&mut self) -> Result<mcu_common::Error> {
        mcu_common::Error::try_from(self.read_register(registers::ERROR)?)
            .map_err(|_| anyhow!("Errors register contained unknown error"))
    }

    fn check_errors(&mut self) -> Result<()> {
        match self.read_register(registers::ERROR)? {
            0 => Ok(()),
            value => match mcu_common::Error::try_from(value) {
                Ok(errors) => bail!("HPS reports error: {:?}", errors),
                Err(_) => bail!("HPS reports error: 0x{:x}", value),
            },
        }
    }
}

pub struct HpsAdapter<I> {
    sink: I,
}

impl<I, E> HpsAdapter<I>
where
    I: i2c::WriteRead<Error = E> + i2c::Write<Error = E> + i2c::Read<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    pub fn new(sink: I) -> Result<Self> {
        let mut hps = Self::without_magic_check(sink);
        hps.wait_ready()?;
        hps.check_magic()?;
        Ok(hps)
    }

    pub fn from_ref(sink: &mut I) -> Result<HpsAdapter<BorrowedI2c<I>>> {
        HpsAdapter::new(BorrowedI2c { i2c: sink })
    }

    pub fn without_magic_check(sink: I) -> Self {
        Self { sink }
    }

    /// Tries reading from the HPS until either we get several successful reads
    /// in a row, or we hit a timeout. This helps ensure that the HPS is up and
    /// running before we try talking to it.
    pub fn wait_ready(&mut self) -> Result<()> {
        let mut buf = [0u8];
        let start = Instant::now();
        let mut success = 0;
        while start.elapsed() < I2C_START_TIMEOUT {
            if self.sink.read(HPS_ADDRESS, &mut buf).is_ok() {
                success += 1;
                if success == 10 {
                    return Ok(());
                }
            } else {
                success = 0;
            }
        }
        self.sink
            .read(HPS_ADDRESS, &mut buf)
            .context("Timeout waiting for HPS to respond on I2C")
    }
}

impl<I, E> Hps for HpsAdapter<I>
where
    I: i2c::WriteRead<Error = E> + i2c::Write<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    fn read_register(&mut self, reg: u8) -> Result<u16> {
        let mut buffer = [0u8; 2];
        self.sink
            .write_read(HPS_ADDRESS, &[register_access_command(reg)], &mut buffer)?;
        Ok(((buffer[0] as u16) << 8) | (buffer[1] as u16))
    }

    fn read_register_bytes(&mut self, reg: u8, length: usize) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; length];
        self.sink
            .write_read(HPS_ADDRESS, &[register_access_command(reg)], &mut buffer)?;
        Ok(buffer)
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

/// This struct wraps a mutable reference to an I2C implementation and
/// implements the I2C traits. This won't be necessary once we're using
/// embedded-hal 1.0 (not yet released), since it has blanket implementations of
/// the I2C traits for references.
pub struct BorrowedI2c<'a, I> {
    i2c: &'a mut I,
}

impl<'a, I, E> i2c::WriteRead for BorrowedI2c<'a, I>
where
    I: i2c::WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Error = E;

    fn write_read(
        &mut self,
        address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        (*self.i2c).write_read(address, bytes, buffer)
    }
}

impl<'a, I, E> i2c::Write for BorrowedI2c<'a, I>
where
    I: i2c::Write<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Error = E;

    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        (*self.i2c).write(address, bytes)
    }
}

impl<'a, I, E> i2c::Read for BorrowedI2c<'a, I>
where
    I: i2c::Read<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Error = E;

    fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        (*self.i2c).read(address, buffer)
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
            Ok(Event::ReadRegister(event)) => {
                assert_eq!(event.register, 42);
                event.respond_u16(99, &mut i2c);
            }
            event => panic!("Unexpected event {:?}", event),
        }
        assert_eq!(i2c.next_event(), Ok(Event::None));
    }
}
