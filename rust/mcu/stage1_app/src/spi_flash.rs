// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use crate::fpga_device::Segment;
use crate::fpga_device::SegmentKind;
use hal::gpio::Analog;
use hal::prelude::OutputPin;
use hal::rcc::Rcc;
use hmac_sha256::Hash;
use log::error;
use log::info;
use mcu_common::Error;
use mcu_common::SPI_BLOCK_SIZE;
use mcu_common::SPI_FLASH_SIZE;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;
use spi_memory::Read;

pub(crate) const PAGE_SIZE: u32 = 256;

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
    if address.saturating_add(data.len() as u32) > SPI_FLASH_SIZE {
        return Err(Error::HostI2cBadRequest);
    }
    // TODO(dml) the whole flash is erased as part of the update sequence, so
    // this is now unnecessary. This is still used for development though so
    // cannot yet be removed. We erase the entire SPI flash prior to writing
    // the first bit of data. Host must send data in blocks that evenly
    // divides this size.
    if address & !(SPI_BLOCK_SIZE - 1) != (address + data.len() as u32 - 1) & !(SPI_BLOCK_SIZE - 1)
    {
        error!("SPI write crosses block boundary");
        return Err(Error::HostI2cBadRequest);
    }
    if address % SPI_BLOCK_SIZE == 0 {
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

pub(crate) fn erase(
    spi_flash: Option<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>>,
) -> Result<(), Error> {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return Err(Error::HostI2cBadRequest);
        }
    };
    info!("Erasing");
    if spi_flash.erase_all().is_err() {
        error!("Failed to erase to SPI flash");
        return Err(Error::SpiFlash);
    } else {
        info!("Erase complete");
    }
    Ok(())
}

pub(crate) fn hash_internal(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
    first_page: u32,
    end_page: u32,
) -> Result<[u8; 32], Error> {
    let mut hasher = Hash::new();
    let mut buf = [0x00; PAGE_SIZE as usize];
    for page in first_page..end_page {
        if spi_flash.read(page * (PAGE_SIZE as u32), &mut buf).is_err() {
            error!("Failed to read back SPI flash");
            return Err(Error::SpiFlash);
        }
        hasher.update(&buf);
    }
    Ok(hasher.finalize())
}

pub(crate) fn hash(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
    segments: &[Segment],
) -> Result<(), Error> {
    let mut first_page: u32 = 0;
    let empty_page = [0xFFu8; PAGE_SIZE as usize];
    for seg in segments {
        let end_page = first_page + seg.length as u32 / PAGE_SIZE;
        match seg.kind {
            SegmentKind::Empty => {
                let mut buf = [0x00; PAGE_SIZE as usize];
                for page in first_page..end_page {
                    if spi_flash
                        .read((page as u32) * (PAGE_SIZE as u32), &mut buf)
                        .is_err()
                    {
                        error!("Failed to read back SPI flash");
                        return Err(Error::SpiFlashNotVerified);
                    }
                }
                // We consider a page to be empty for the purposes of hashing if
                // either it is truly empty (all 0xff) or it contains our test
                // pattern, which we sometimes write into the last 1MB of flash.
                if empty_page != buf && crate::testing::TEST_PATTERN != buf {
                    error!("Expected SPI flash register to be empty");
                    return Err(Error::SpiFlashNotVerified);
                }
            }
            SegmentKind::DataWithHash(expected) => {
                if expected != hash_internal(spi_flash, first_page, end_page).unwrap() {
                    error!("SPI flash hash does not match");
                    return Err(Error::SpiFlashNotVerified);
                }
            }
        }
        first_page = end_page;
    }
    Ok(())
}

/// Uninitializes the SPI interface to the SPI flash. This is used when we're
/// done talking to the SPI flash and ready for the FPGA to take over. We turn
/// off the SPI peripheral and reset the pin states to floating inputs.
pub(crate) fn uninitialize_spi(
    flash: Flash<board::FlashSpiControl, board::FlashSpiCs>,
    _rcc: &mut Rcc,
) {
    // safety: If spi_memory::Flash supported a method to release its resources
    // back to us, we'd use it here, but it doesn't. But we own it, so we drop
    // it then steal its resources.
    drop(flash);
    let flash = unsafe { SpiFlash::steal() };
    flash.cs.into_floating_input();

    // Turn off the SPI peripheral.

    // safety: We need access to the spi1en bit, which is part of RCC, which is
    // owned by Rcc. We hold an exclusive reference to Rcc. If we can add
    // support to the HAL crate for turning peripherals on and off, then this
    // could be improved.
    let p = unsafe { hal::stm32::Peripherals::steal() };
    p.RCC.apbenr2.modify(|_, w| w.spi1en().clear_bit());

    // Reset pins as floating inputs.

    // safety: Ownership of these pins was passed to FlashSpiController when it
    // was constructed, but it had no further use for them, so it dropped them.
    // We reconjur them out of thin air.
    unsafe {
        let clk: hal::gpio::gpioa::PA5<Analog> = core::mem::transmute(());
        clk.into_floating_input();
        let cipo: hal::gpio::gpioa::PA6<Analog> = core::mem::transmute(());
        cipo.into_floating_input();
        let copi: hal::gpio::gpioa::PA7<Analog> = core::mem::transmute(());
        copi.into_floating_input();
    }
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
    pub unsafe fn steal() -> Self {
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

    pub(crate) fn erase_block(&mut self, address: u32) -> Result<(), SpiFlashError> {
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
