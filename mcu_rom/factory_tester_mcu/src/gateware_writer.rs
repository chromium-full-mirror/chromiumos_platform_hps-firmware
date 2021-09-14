// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::FlashSpi;
use crate::FlashSpiCs;
use hal::stm32::TIM2;
use hal::timer::stopwatch::Stopwatch;
use log::error;
use log::info;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;
use spi_memory::Read;

const COMPRESSED_GATEWARE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/compressed_gateware.bit"));

const PAGE_SIZE: u32 = 256;

pub(crate) fn write_gateware(
    flash: &mut Flash<FlashSpi, FlashSpiCs>,
    stopwatch: &mut Stopwatch<TIM2>,
) -> Result<(), Error> {
    let id = flash.read_jedec_id().map_err(convert_spi_flash_error)?;
    if id.mfr_code() != 0xC8 || id.device_id() != [0x60, 0x18] {
        return Err(Error::UnexpectedSpiFlashId);
    }

    let start_time = stopwatch.now();

    // In the common case when the SPI flash is empty, this check will fail
    // almost instantly, so costs us nothing. However if the test is being rerun
    // and the SPI flash already has the correct contents, then this saves us
    // rewriting and more importantly erasing, which is quite time consuming.
    if verify_spi_flash(flash).is_ok() {
        info!("SPI flash already has correct contents. Skipping write");
        return Ok(());
    }

    // Most of the time, the SPI flash will be empty, so we optimistically
    // assume it is and just write without erasing. Then if validation fails, we
    // do a full erase and write again.
    let bytes_written = write_without_erase(flash)?;

    info!(
        "Wrote {} bytes to SPI flash in {}ms",
        bytes_written,
        stopwatch.elapsed(start_time).0 / 1000
    );

    let start_time = stopwatch.now();
    if verify_spi_flash(flash).is_err() {
        flash.erase_all().map_err(convert_spi_flash_error)?;
        write_without_erase(flash)?;
        verify_spi_flash(flash)?;
    }
    info!(
        "Verified SPI flash in {}ms",
        stopwatch.elapsed(start_time).0 / 1000
    );

    Ok(())
}

fn write_without_erase(flash: &mut Flash<FlashSpi, FlashSpiCs>) -> Result<u32, Error> {
    info!("Writing SPI flash");
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
        flash
            .write_bytes(address, &mut buffer)
            .map_err(convert_spi_flash_error)?;
        address += PAGE_SIZE;
    }
    Ok(address)
}

pub(crate) fn verify_spi_flash(flash: &mut Flash<FlashSpi, FlashSpiCs>) -> Result<(), Error> {
    let mut address = 0;
    let mut gateware = gateware_compress::Decompressor::new(COMPRESSED_GATEWARE);
    let mut buffer = [0u8; PAGE_SIZE as usize];
    loop {
        flash
            .read(address, &mut buffer)
            .map_err(convert_spi_flash_error)?;
        for actual in buffer {
            if let Some(expected) = gateware.next() {
                if actual != expected {
                    return Err(Error::SpiFlashCommunication);
                }
            } else {
                return Ok(());
            }
        }
        address += PAGE_SIZE;
    }
}

fn convert_spi_flash_error(error: spi_memory::Error<FlashSpi, FlashSpiCs>) -> Error {
    error!("SPI flash error: {:?}", error);
    Error::SpiFlashCommunication
}
