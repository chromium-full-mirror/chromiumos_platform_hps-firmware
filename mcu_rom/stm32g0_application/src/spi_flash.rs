// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use hal::timer::stopwatch::Stopwatch;
use hmac_sha256::Hash;
use rtt_target::rprint;
use rtt_target::rprintln;
use spi_memory::series25::Flash;
use spi_memory::BlockDevice;
use spi_memory::Read;

const SECTOR_SIZE: u32 = 4096;
const PAGE_SIZE: u32 = 256;

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

pub(crate) fn debug_read(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    address: u32,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return;
        }
    };
    let mut buffer = [0u8; 16];
    if spi_flash.read(address, &mut buffer).is_err() {
        rprintln!("Failed to read SPI flash");
        return;
    }
    rprintln!("SPI flash bytes");
    for v in &buffer {
        rprint!("{}, ", v);
    }
    rprintln!("===");
}

pub(crate) fn erase(spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return;
        }
    };
    rprintln!("Erasing");
    if spi_flash.erase_all().is_err() {
        rprintln!("Failed to erase to SPI flash");
    }
    rprintln!("Erase complete");
}

pub(crate) fn debug_write(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    address: u32,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return;
        }
    };
    let mut data = [
        0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];
    rprintln!("Writing");
    if spi_flash.write_bytes(address, &mut data).is_err() {
        rprintln!("Failed to write to SPI flash");
    }
    rprintln!("Successfully wrote to SPI flash");
}

pub(crate) fn hash(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    num_pages: u16,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return;
        }
    };
    let elapsed_us = stopwatch.trace(|| {
        let mut hasher = Hash::new();
        let mut buf = [0x00; PAGE_SIZE as usize];
        for page in 0..num_pages {
            let _ = spi_flash.read((page as u32) * (PAGE_SIZE as u32), &mut buf);
            hasher.update(&buf);
        }
        let hash = hasher.finalize();
        rprintln!("Hash of first {} pages was: {:?}", num_pages, hash);
    });
    rprintln!("Hashing took {}us", elapsed_us.0);
}
