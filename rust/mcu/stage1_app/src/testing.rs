// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Code for helping test that things are working properly.

use crate::board;
use mcu_common::Error;
use mcu_common::SPI_FLASH_SIZE;
use mcu_common::SPI_TEST_DATA_OFFSET;
use spi_memory::series25::Flash;
use spi_memory::Read;

pub(crate) fn write_spi_test_data(
    spi_flash: &mut Flash<board::FlashSpiControl, board::FlashSpiCs>,
) -> Result<(), Error> {
    if verify_spi_test_data(spi_flash).is_ok() {
        return Ok(());
    }
    let mut buf = [0u8; 256];
    for (offset, byte) in buf.iter_mut().enumerate() {
        *byte = offset as u8;
    }
    let mut address = SPI_TEST_DATA_OFFSET;
    while address < SPI_FLASH_SIZE {
        crate::spi_flash::write(Some(spi_flash), address, &buf)?;
        address += buf.len() as u32;
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
