// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use rtt_target::rprintln;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;

/// Write's `data` to `address` in the SPI flash. `data` is updated based on
/// response from the SPI flash. We don't actually use this returned data, but
/// we don't get any choice about receiving the response.
pub(crate) fn write(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    address: u32,
    data: &mut [u8],
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available, cannot write");
            return;
        }
    };
    if data.len() > 256 {
        rprintln!("SPI flash must currently be written in blocks of 256 bytes at a time");
        return;
    }
    // We erase the entire SPI flash prior to writing the first bit of data.
    // Host must send data in blocks that evenly divides this size.
    const SECTOR_SIZE: u32 = 4096;
    const PAGE_SIZE: u32 = 256;
    if address & !(PAGE_SIZE - 1) != (address + data.len() as u32 - 1) & !(PAGE_SIZE - 1) {
        rprintln!("SPI write crosses page boundary");
        return;
    }
    if address % SECTOR_SIZE == 0 {
        if spi_flash.erase_sectors(address, 1).is_err() {
            rprintln!("Failed to erase to sector");
        }
    }
    if spi_flash.write_bytes(address, data).is_err() {
        rprintln!("Failed to write to SPI flash");
    }
}
