// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::debug_commands;
use crate::DataSink;
use crate::DataSource;
use anyhow::bail;
use anyhow::Result;
use crc::Crc;
use mcu_common::McuDebugCommand;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

const PAGE_SIZE: usize = 256;
const SECTOR_SIZE: usize = 4096;
const PAGES_PER_SECTOR: usize = SECTOR_SIZE / PAGE_SIZE;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);
const MAX_OUTSTANDING_COMMANDS: usize = 10;

pub(crate) fn write_spi_flash(
    command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    start_offset: usize,
    spi_flash_file: &Path,
) -> Result<()> {
    assert!(start_offset % PAGE_SIZE == 0);
    if let (Some(command_channel), Some(cmd_response_channel)) =
        (command_channel.as_mut(), cmd_response_channel.as_mut())
    {
        let mut file_bytes = std::fs::read(spi_flash_file)?;

        // Pad with 0 up to end of current sector boundary.
        file_bytes.resize((file_bytes.len() + SECTOR_SIZE - 1) & !(SECTOR_SIZE - 1), 0);

        let sectors_to_write =
            get_sectors_to_write(&file_bytes, command_channel, cmd_response_channel)?;
        let total_sectors = file_bytes.len() / SECTOR_SIZE;
        println!(
            "Need to write {} of {} sectors",
            sectors_to_write.len(),
            total_sectors
        );
        let start_page = (start_offset / PAGE_SIZE) as u16;

        let start = std::time::Instant::now();
        let mut pages_written = 0;
        let mut outstanding_commands = 0;
        let total_pages = sectors_to_write.len() * PAGES_PER_SECTOR;
        for sector_num in sectors_to_write {
            for page_offset in 0..PAGES_PER_SECTOR {
                let page_num = sector_num * PAGES_PER_SECTOR + page_offset;
                let chunk = &file_bytes[page_num * PAGE_SIZE..(page_num + 1) * PAGE_SIZE];
                let command = debug_commands::DebugCommand {
                    code: McuDebugCommand::WriteSpiFlash.into(),
                    arg: page_num as u16 + start_page,
                };
                command_channel.write(&command.to_bytes())?;
                command_channel.write(chunk)?;
                outstanding_commands += 1;
                if outstanding_commands >= MAX_OUTSTANDING_COMMANDS {
                    wait_cmd_response(cmd_response_channel)?;
                    outstanding_commands -= 1;
                }
                pages_written += 1;
                if pages_written % 10 == 0 {
                    println!(
                        "{:0.1}% complete. {:0.1}KiB/s",
                        pages_written as f64 * 100f64 / total_pages as f64,
                        ((pages_written * PAGE_SIZE) as f64 / start.elapsed().as_secs_f64())
                            / 1024f64
                    );
                }
            }
        }

        println!(
            "Finished sending data, waiting for last {} writes to complete",
            outstanding_commands
        );

        for _ in 0..outstanding_commands {
            wait_cmd_response(cmd_response_channel)?;
        }

        let seconds = start.elapsed().as_secs_f64();
        println!(
            "Wrote {} pages to SPI flash in {:0.1} seconds ({:0.1}KiB/s)",
            pages_written,
            seconds,
            ((total_pages * PAGE_SIZE) as f64 / seconds) / 1024f64
        );
    } else {
        bail!("No command channel available, command not sent.");
    }
    Ok(())
}

struct SectorInfo {
    sector_num: usize,
    desired_crc: u32,
}

fn get_sectors_to_write(
    file_bytes: &[u8],
    command_channel: &mut Box<dyn DataSink>,
    cmd_response_channel: &mut Box<dyn DataSource>,
) -> Result<Vec<usize>> {
    let mut need_to_write = Vec::new();
    let mut outstanding_sectors = VecDeque::new();
    let mut work_iter = file_bytes.chunks_exact(SECTOR_SIZE).enumerate();
    let total_sectors = file_bytes.len() / SECTOR_SIZE;
    let mut sectors_checked = 0;
    loop {
        let work_exhausted;
        if let Some((sector_num, sector_bytes)) = work_iter.next() {
            let command = debug_commands::DebugCommand {
                code: McuDebugCommand::SpiFlashCrc.into(),
                arg: sector_num as u16,
            };
            command_channel.write(&command.to_bytes())?;
            let mut digest = CRC.digest();
            digest.update(sector_bytes);
            outstanding_sectors.push_back(SectorInfo {
                sector_num,
                desired_crc: digest.finalize(),
            });
            work_exhausted = false;
        } else {
            work_exhausted = true;
            if outstanding_sectors.is_empty() {
                break;
            }
        }
        if work_exhausted || outstanding_sectors.len() >= MAX_OUTSTANDING_COMMANDS {
            let crc = u32::from_le_bytes(wait_cmd_response(cmd_response_channel)?);
            sectors_checked += 1;
            if let Some(info) = outstanding_sectors.pop_front() {
                if sectors_checked % 10 == 0 {
                    println!(
                        "CRC checked {} of {} sectors",
                        sectors_checked, total_sectors,
                    );
                }
                if crc != info.desired_crc {
                    need_to_write.push(info.sector_num);
                }
            }
        }
    }
    Ok(need_to_write)
}

fn wait_cmd_response(cmd_response_channel: &mut Box<dyn DataSource>) -> Result<[u8; 4]> {
    // If we're using OpenOCD, then reads on this channel should have been set
    // to block. If we're using an ST-Link, then we'll effectively end up
    // polling. In either case, polling will occur, with OpenOCD doing the
    // polling if it's being used.
    let mut bytes_read = 0;
    let mut response = [0u8; 4];
    loop {
        bytes_read += cmd_response_channel.read(&mut response[bytes_read..])?;
        if bytes_read == response.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(response)
}
