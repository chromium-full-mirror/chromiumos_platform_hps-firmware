// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Code for helping test that things are working properly.

use crate::board;
use mcu_common::Error;
use mcu_common::SPI_FLASH_SIZE;
use mcu_common::SPI_TEST_DATA_OFFSET;
use spi_memory::series25::Flash;
use spi_memory::Read;

/// The test pattern that we write to the last 1MB of SPI flash. This is the
/// numbers 0 through 255 inclusive.
pub(crate) const TEST_PATTERN: [u8; 256] = generate_test_pattern();

pub(crate) fn write_spi_test_data(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
) -> Result<(), Error> {
    if verify_spi_test_data(spi_flash).is_ok() {
        return Ok(());
    }
    let mut address = SPI_TEST_DATA_OFFSET;
    while address < SPI_FLASH_SIZE {
        crate::spi_flash::write(Some(spi_flash), address, &TEST_PATTERN)?;
        address += TEST_PATTERN.len() as u32;
    }
    Ok(())
}

pub(crate) fn verify_spi_test_data(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
) -> Result<(), Error> {
    let mut buf = [0u8; 256];
    let mut address = SPI_TEST_DATA_OFFSET;
    while address < SPI_FLASH_SIZE {
        spi_flash
            .read(address, &mut buf)
            .map_err(|_| Error::SpiFlash)?;
        for (offset, byte) in buf.iter_mut().enumerate() {
            if *byte != offset as u8 {
                return Err(Error::SpiFlash);
            }
        }
        address += buf.len() as u32;
    }
    Ok(())
}

/// Returns what a page of test data looks like. The numbers [0...255].
const fn generate_test_pattern() -> [u8; 256] {
    let mut page = [0u8; 256];
    let mut offset = 0;
    while offset < page.len() {
        page[offset] = offset as u8;
        offset += 1;
    }
    page
}
