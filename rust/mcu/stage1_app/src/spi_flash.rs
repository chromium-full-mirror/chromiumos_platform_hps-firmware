// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use crc::Crc;
use hal::prelude::OutputPin;
use hal::timer::stopwatch::Stopwatch;
use hmac_sha256::Hash;
use log::error;
use log::info;
use mcu_common::Error;
use mcu_common::SPI_FLASH_SIZE;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;
use spi_memory::Read;

const PAGE_SIZE: u32 = 256;
const BLOCK_SIZE: u32 = 64 * 1024;

/// Write's `data` to `address` in the SPI flash. `data` is updated based on
/// response from the SPI flash. We don't actually use this returned data, but
/// we don't get any choice about receiving the response.
pub(crate) fn write(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
    address: u32,
    data: &[u8],
) -> Result<(), Error> {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available, cannot write");
            return Err(Error::HostI2cBadRequest);
        }
    };
    if data.len() > PAGE_SIZE as usize {
        error!("SPI flash must currently be written in blocks of at most 256 bytes at a time");
        return Err(Error::HostI2cBadRequest);
    }
    // TODO(dml) the whole flash is erased as part of the update sequence, so
    // this is now unnecessary. This is still used for development though so
    // cannot yet be removed. We erase the entire SPI flash prior to writing
    // the first bit of data. Host must send data in blocks that evenly
    // divides this size.
    if address & !(BLOCK_SIZE - 1) != (address + data.len() as u32 - 1) & !(BLOCK_SIZE - 1) {
        error!("SPI write crosses block boundary");
        return Err(Error::HostI2cBadRequest);
    }
    if address % BLOCK_SIZE == 0 {
        // The spi-memory crate doesn't support erasing blocks, so we do it
        // ourselves. https://github.com/jonas-schievink/spi-memory/issues/37
        let mut spi = unsafe { SpiFlash::steal() };
        if spi.erase_block(address).is_err() {
            error!("Failed to erase to block");
            return Err(Error::SpiFlash);
        }
    }
    let mut buffer = [0u8; PAGE_SIZE as usize];
    let buffer = &mut buffer[..data.len()];
    buffer.copy_from_slice(data);
    if spi_flash.write_bytes(address, buffer).is_err() {
        error!("Failed to write to SPI flash");
        return Err(Error::SpiFlash);
    }

    if spi_flash.read(address, buffer).is_err() {
        error!("Failed to read back SPI flash");
        return Err(Error::SpiFlash);
    }

    if buffer != data {
        error!(
            "Read back of data showed mismatch after writing 0x{:x} bytes at address 0x{:x}",
            data.len(),
            address
        );
        return Err(Error::SpiFlash);
    }

    Ok(())
}

pub(crate) fn enable_quad_spi(
    _flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
) -> Result<(), SpiFlashError> {
    // Safety: We already have exclusive use of the flash.
    let mut flash = unsafe { SpiFlash::steal() };
    flash.enable_quad_spi()
}

pub(crate) fn debug_read(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
    address: u32,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    let mut buffer = [0u8; 16];
    if spi_flash.read(address, &mut buffer).is_err() {
        error!("Failed to read SPI flash");
        return;
    }
    info!("SPI flash bytes");
    for v in &buffer {
        info!("{}, ", v);
    }
}

pub(crate) fn erase(spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    info!("Erasing");
    if spi_flash.erase_all().is_err() {
        error!("Failed to erase to SPI flash");
    } else {
        info!("Erase complete");
    }
}

fn hash_internal(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
    num_pages: u32,
) -> Result<[u8; 32], Error> {
    let mut hasher = Hash::new();
    let mut buf = [0x00; PAGE_SIZE as usize];
    for page in 0..num_pages {
        if spi_flash
            .read((page as u32) * (PAGE_SIZE as u32), &mut buf)
            .is_err()
        {
            error!("Failed to read back SPI flash");
            return Err(Error::SpiFlash);
        }
        hasher.update(&buf);
    }
    Ok(hasher.finalize())
}

pub(crate) fn hash(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
) -> Result<[u8; 32], Error> {
    hash_internal(spi_flash, SPI_FLASH_SIZE / PAGE_SIZE)
}

pub(crate) fn time_hash(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
    num_pages: u32,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    let elapsed_us = stopwatch.trace(|| {
        let hash = hash_internal(spi_flash, num_pages);
        info!("Hash of first {} pages was: {:?}", num_pages, hash);
    });
    info!("Hashing took {}us", elapsed_us.0);
}

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);

pub(crate) fn crc_sector(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
    block: u16,
) -> [u8; 4] {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return [0, 0, 0, 0];
        }
    };
    let mut digest = CRC.digest();
    let mut buf = [0x00; PAGE_SIZE as usize];
    const PAGES_PER_BLOCK: u32 = BLOCK_SIZE / PAGE_SIZE;
    for page_offset in 0..PAGES_PER_BLOCK {
        let _ = spi_flash.read(
            (block as u32 * PAGES_PER_BLOCK + page_offset) * PAGE_SIZE,
            &mut buf,
        );
        digest.update(&buf);
    }
    digest.finalize().to_le_bytes()
}

pub(crate) fn test_read_speed(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    info!("Reading...");
    const READ_SIZE: u32 = 1024 * 1024;
    let mut buf = [0u8; PAGE_SIZE as usize];
    let elapsed_us = stopwatch.trace(|| {
        for page in 0..(READ_SIZE / PAGE_SIZE) {
            let _ = spi_flash.read(page * PAGE_SIZE, &mut buf);
        }
    });
    info!("Reading 1MB took {}us", elapsed_us.0);
}

pub(crate) fn test_write_speed(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    info!("Writing...");
    const READ_SIZE: u32 = 1024 * 1024;
    let mut buf = [42u8; PAGE_SIZE as usize];
    let mut erase_time = 0;
    let mut write_time = 0;
    for page in 0..(READ_SIZE / PAGE_SIZE) {
        let address = 9 * 1024 * 1024 + page * PAGE_SIZE;
        if address % BLOCK_SIZE == 0 {
            erase_time += stopwatch
                .trace(|| {
                    // safety: We hold a mutable reference to Flash, which owns
                    // the same peripheral.
                    let mut spi = unsafe { SpiFlash::steal() };
                    let _ = spi.erase_block(address);
                })
                .0;
        }
        write_time += stopwatch
            .trace(|| {
                let _ = spi_flash.write_bytes(address, &mut buf);
            })
            .0;
    }
    info!(
        "Erasing 1MB took {}ms. Writing 1MB took {}ms",
        erase_time / 1000,
        write_time / 1000
    );
}

pub(crate) enum SpiFlashError {
    Gpio,
    TransferFailed,
    FailedToEnableQuadSpi,
}

pub(crate) struct SpiFlash {
    pub(crate) spi: board::FlashSpiControl,
    pub(crate) cs: board::FlashSpiCs,
}

enum Command {
    WriteStatusRegister = 0x1,
    ReadStatusRegisterLow = 0x5,
    WriteEnable = 0x6,
    ReadStatusRegisterHigh = 0x35,
    EraseBlock = 0xd8,
}

enum StatusLow {
    WriteInProgress = 0x1,
}

enum StatusHigh {
    QuadEnable = 0x2,
}

impl SpiFlash {
    pub(crate) fn read_status(&mut self) -> Result<u16, SpiFlashError> {
        Ok(u16::from_le_bytes([
            self.read_reg(Command::ReadStatusRegisterLow)?,
            self.read_reg(Command::ReadStatusRegisterHigh)?,
        ]))
    }

    pub(crate) fn enable_quad_spi(&mut self) -> Result<(), SpiFlashError> {
        let high = self.read_reg(Command::ReadStatusRegisterHigh)?;
        if high & StatusHigh::QuadEnable as u8 != 0 {
            // Already enabled
            return Ok(());
        }

        let low = self.read_reg(Command::ReadStatusRegisterLow)?;

        self.write_enable()?;
        let mut buf = [
            Command::WriteStatusRegister as u8,
            low,
            high | StatusHigh::QuadEnable as u8,
        ];
        self.transfer(&mut buf)?;

        self.wait_write_complete()?;

        // Read back to verify that the bit is now set.
        let high = self.read_reg(Command::ReadStatusRegisterHigh)?;

        if high & StatusHigh::QuadEnable as u8 == 0 {
            return Err(SpiFlashError::FailedToEnableQuadSpi);
        }

        Ok(())
    }

    /// safety: You must hold a mutable reference Spi and must not use it while
    /// you're using the returned result. i.e. SPI must already be initialized
    /// and you must not access it via other means concurrently with using the
    /// returned interface.
    unsafe fn steal() -> Self {
        // safety: We're transmuting a zero-sized type, so there's no state to
        // initialize in the returned value. All initialization lives in the
        // peripheral itself and it's up to the caller to ensure that has
        // already occurred.
        core::mem::transmute(())
    }

    /// Waits until the SPI flash stops reporting that a write is in progress.
    fn wait_write_complete(&mut self) -> Result<(), SpiFlashError> {
        while self.read_reg(Command::ReadStatusRegisterLow)? & (StatusLow::WriteInProgress as u8)
            != 0
        {}
        Ok(())
    }

    /// Reads a single byte register.
    fn read_reg(&mut self, command: Command) -> Result<u8, SpiFlashError> {
        let mut buf = [command as u8, 0];
        self.transfer(&mut buf)?;
        Ok(buf[1])
    }

    fn write_enable(&mut self) -> Result<(), SpiFlashError> {
        self.transfer(&mut [Command::WriteEnable as u8])
    }

    fn transfer(&mut self, buf: &mut [u8]) -> Result<(), SpiFlashError> {
        use embedded_hal::blocking::spi::Transfer;
        self.cs.set_low().map_err(|_| SpiFlashError::Gpio)?;
        self.spi
            .transfer(buf)
            .map_err(|_| SpiFlashError::TransferFailed)?;
        self.cs.set_high().map_err(|_| SpiFlashError::Gpio)?;
        Ok(())
    }

    fn erase_block(&mut self, address: u32) -> Result<(), SpiFlashError> {
        self.write_enable()?;
        let mut buf = [
            Command::EraseBlock as u8,
            (address >> 16) as u8,
            (address >> 8) as u8,
            (address & 0xff) as u8,
        ];
        self.transfer(&mut buf)?;
        self.wait_write_complete()?;
        Ok(())
    }
}
