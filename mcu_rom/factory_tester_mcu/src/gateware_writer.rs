// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::FlashSpi;
use crate::FlashSpiCs;
use crate::Stopwatch;
use log::error;
use log::info;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;
use spi_memory::Read;

const COMPRESSED_GATEWARE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/compressed_gateware.bit"));

const PAGE_SIZE: u32 = 256;

enum SpiFlashStatus {
    /// The SPI flash already contains the correct gateware.
    Verified,
    /// The SPI flash is blank. Note, we only check the first page, so if
    /// something was written elsewhere, but nothing was written to the first
    /// page, we'd incorrectly think it was blank, but we never do that.
    Blank,
    /// There's something already written, but it's an old or incorrect version.
    UpdateNeeded,
}

pub(crate) fn write_gateware(
    flash: &mut Flash<FlashSpi, FlashSpiCs>,
    stopwatch: &mut Stopwatch,
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
    match verify_spi_flash(flash)? {
        SpiFlashStatus::Verified => {
            info!("SPI flash already has correct contents. Skipping write");
            return Ok(());
        }
        SpiFlashStatus::Blank => {}
        SpiFlashStatus::UpdateNeeded => {
            flash.erase_all().map_err(convert_spi_flash_error)?;
        }
    }

    let bytes_written = write_without_erase(flash)?;

    info!(
        "Wrote {} bytes to SPI flash in {}ms",
        bytes_written,
        stopwatch.elapsed(start_time).0 / 1000
    );

    // We don't verify what we wrote in order to save time. It should almost
    // always be correct. If it's incorrect and that causes something else to
    // fail, then if the test gets rerun, the verification above should detect
    // the mismatch and erase the bad contents.

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

fn verify_spi_flash(flash: &mut Flash<FlashSpi, FlashSpiCs>) -> Result<SpiFlashStatus, Error> {
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
                    if address == 0 && buffer == [0xff; PAGE_SIZE as usize] {
                        // We never write to the SPI flash without first writing
                        // to the first page, so if the first page is blank, we
                        // assume the SPI flash is blank. This allows us to save
                        // the time that would be required to check the whole
                        // ~350KB to see if it's blank.
                        return Ok(SpiFlashStatus::Blank);
                    }
                    return Ok(SpiFlashStatus::UpdateNeeded);
                }
            } else {
                return Ok(SpiFlashStatus::Verified);
            }
        }
        address += PAGE_SIZE;
    }
}

fn convert_spi_flash_error(error: spi_memory::Error<FlashSpi, FlashSpiCs>) -> Error {
    error!("SPI flash error: {:?}", error);
    Error::SpiFlashCommunication
}
