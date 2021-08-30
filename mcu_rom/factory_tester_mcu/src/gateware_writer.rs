// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::rprintln;
use crate::Error;
use crate::FlashSpi;
use crate::FlashSpiCs;
use hal::stm32::TIM2;
use hal::timer::stopwatch::Stopwatch;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;
use spi_memory::Read;

const COMPRESSED_GATEWARE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/compressed_gateware.bit"));

const SECTOR_SIZE: u32 = 4096;
const PAGE_SIZE: u32 = 256;

pub(crate) fn write_gateware(
    flash: &mut Flash<FlashSpi, FlashSpiCs>,
    stopwatch: &mut Stopwatch<TIM2>,
) -> Result<(), Error> {
    let id = flash.read_jedec_id()?;
    if id.mfr_code() != 0xC8 || id.device_id() != [0x60, 0x18] {
        return Err(Error::UnexpectedSpiFlashId);
    }

    let start_time = stopwatch.now();

    // In the common case when the SPI flash is empty, this check will fail
    // almost instantly, so costs us nothing. However if the test is being rerun
    // and the SPI flash already has the correct contents, then this saves us
    // rewriting and more importantly erasing, which is quite time consuming.
    if verify_spi_flash(flash).is_ok() {
        rprintln!("SPI flash already has correct contents. Skipping write");
        return Ok(());
    }

    rprintln!("Writing SPI flash");
    let mut address = 0;
    let mut gateware = gateware_compress::Decompressor::new(COMPRESSED_GATEWARE);
    let mut buffer = [0u8; PAGE_SIZE as usize];
    let mut done = false;
    while !done {
        buffer.fill_with(|| match gateware.next() {
            Some(x) => x,
            None => {
                done = true;
                // Pad out to the end of the last page with zeros.
                0
            }
        });
        // Erase sector when we reach the start of each sector.
        if address % SECTOR_SIZE == 0 {
            flash.erase_sectors(address, 1)?;
        }
        flash.write_bytes(address, &mut buffer)?;
        address += PAGE_SIZE;
    }

    rprintln!(
        "Wrote {} bytes to SPI flash in {}ms",
        address,
        stopwatch.elapsed(start_time).0 / 1000
    );

    let start_time = stopwatch.now();
    verify_spi_flash(flash)?;
    rprintln!(
        "Verified SPI flash in {}ms",
        stopwatch.elapsed(start_time).0 / 1000
    );

    Ok(())
}

pub(crate) fn verify_spi_flash(flash: &mut Flash<FlashSpi, FlashSpiCs>) -> Result<(), Error> {
    let mut address = 0;
    let mut gateware = gateware_compress::Decompressor::new(COMPRESSED_GATEWARE);
    let mut buffer = [0u8; PAGE_SIZE as usize];
    loop {
        flash.read(address, &mut buffer)?;
        for actual in buffer {
            if let Some(expected) = gateware.next() {
                if actual != expected {
                    return Err(Error::SpiFlashValidateFailed);
                }
            } else {
                return Ok(());
            }
        }
        address += PAGE_SIZE;
    }
}

impl From<spi_memory::Error<FlashSpi, FlashSpiCs>> for Error {
    fn from(error: spi_memory::Error<FlashSpi, FlashSpiCs>) -> Self {
        rprintln!("SPI flash error: {:?}", error);
        Error::SpiFlashError
    }
}
