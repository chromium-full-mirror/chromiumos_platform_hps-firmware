// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use crc::Crc;
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

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);

pub(crate) fn crc_sector(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    sector: u16,
) -> [u8; 4] {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return [0, 0, 0, 0];
        }
    };
    let mut digest = CRC.digest();
    let mut buf = [0x00; PAGE_SIZE as usize];
    const PAGES_PER_SECTOR: u32 = SECTOR_SIZE / PAGE_SIZE;
    for page_offset in 0..PAGES_PER_SECTOR {
        let _ = spi_flash.read(
            (sector as u32 * PAGES_PER_SECTOR + page_offset) * PAGE_SIZE,
            &mut buf,
        );
        digest.update(&buf);
    }
    digest.finalize().to_le_bytes()
}

pub(crate) fn test_read_speed(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return;
        }
    };
    rprintln!("Reading...");
    const READ_SIZE: u32 = 1024 * 1024;
    let mut buf = [0u8; PAGE_SIZE as usize];
    let elapsed_us = stopwatch.trace(|| {
        for page in 0..(READ_SIZE / PAGE_SIZE) {
            let _ = spi_flash.read(page * PAGE_SIZE, &mut buf);
        }
    });
    rprintln!("Reading 1MB took {}us", elapsed_us.0);
}

pub(crate) fn test_write_speed(
    spi_flash: &mut Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            rprintln!("SPI flash not available");
            return;
        }
    };
    rprintln!("Writing...");
    const READ_SIZE: u32 = 1024 * 1024;
    let mut buf = [42u8; PAGE_SIZE as usize];
    let mut erase_time = 0;
    let mut write_time = 0;
    for page in 0..(READ_SIZE / PAGE_SIZE) {
        let address = 9 * 1024 * 1024 + page * PAGE_SIZE;
        if address % SECTOR_SIZE == 0 {
            erase_time += stopwatch
                .trace(|| {
                    let _ = spi_flash.erase_sectors(address, 1);
                })
                .0;
        }
        write_time += stopwatch
            .trace(|| {
                let _ = spi_flash.write_bytes(address, &mut buf);
            })
            .0;
    }
    rprintln!(
        "Erasing 1MB took {}us. Writing 1MB took {}us",
        erase_time,
        write_time
    );
}
