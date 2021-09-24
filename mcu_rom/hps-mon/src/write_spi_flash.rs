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
const BLOCK_SIZE: usize = 64 * 1024;
const PAGES_PER_BLOCK: usize = BLOCK_SIZE / PAGE_SIZE;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);
const MAX_OUTSTANDING_COMMANDS: usize = 28;

pub(crate) fn write_spi_flash(
    command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    start_offset: usize,
    spi_flash_file: &Path,
) -> Result<()> {
    assert!(start_offset % BLOCK_SIZE == 0);
    if let (Some(command_channel), Some(cmd_response_channel)) =
        (command_channel.as_mut(), cmd_response_channel.as_mut())
    {
        let mut file_bytes = std::fs::read(spi_flash_file)?;

        // Pad with 0 up to end of current block boundary.
        file_bytes.resize((file_bytes.len() + BLOCK_SIZE - 1) & !(BLOCK_SIZE - 1), 0);

        let blocks_to_write = get_blocks_to_write(
            &file_bytes,
            command_channel,
            start_offset / BLOCK_SIZE,
            cmd_response_channel,
        )?;
        let total_blocks = file_bytes.len() / BLOCK_SIZE;
        println!(
            "Need to write {} of {} blocks",
            blocks_to_write.len(),
            total_blocks
        );

        let start = std::time::Instant::now();
        let mut pages_written = 0;
        let mut outstanding_commands = 0;
        let total_pages = blocks_to_write.len() * PAGES_PER_BLOCK;
        for block_info in blocks_to_write {
            for page_offset in 0..PAGES_PER_BLOCK {
                let page_num = block_info.block_num * PAGES_PER_BLOCK + page_offset;
                let chunk =
                    &block_info.bytes[page_offset * PAGE_SIZE..(page_offset + 1) * PAGE_SIZE];
                let command = debug_commands::DebugCommand {
                    code: McuDebugCommand::WriteSpiFlash.into(),
                    arg: page_num as u16,
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

struct BlockInfo<'a> {
    block_num: usize,
    desired_crc: u32,
    bytes: &'a [u8],
}

fn get_blocks_to_write<'a>(
    file_bytes: &'a [u8],
    command_channel: &mut Box<dyn DataSink>,
    start_block: usize,
    cmd_response_channel: &mut Box<dyn DataSource>,
) -> Result<Vec<BlockInfo<'a>>> {
    let mut need_to_write = Vec::new();
    let mut outstanding_blocks = VecDeque::new();
    let mut work_iter = file_bytes.chunks_exact(BLOCK_SIZE).enumerate();
    let total_blocks = file_bytes.len() / BLOCK_SIZE;
    let mut blocks_checked = 0;
    loop {
        let work_exhausted;
        if let Some((block_offset, block_bytes)) = work_iter.next() {
            let block_num = start_block + block_offset;
            let command = debug_commands::DebugCommand {
                code: McuDebugCommand::SpiFlashCrc.into(),
                arg: block_num as u16,
            };
            command_channel.write(&command.to_bytes())?;
            let mut digest = CRC.digest();
            digest.update(block_bytes);
            outstanding_blocks.push_back(BlockInfo {
                block_num,
                desired_crc: digest.finalize(),
                bytes: block_bytes,
            });
            work_exhausted = false;
        } else {
            work_exhausted = true;
            if outstanding_blocks.is_empty() {
                break;
            }
        }
        if work_exhausted || outstanding_blocks.len() >= MAX_OUTSTANDING_COMMANDS {
            let crc = u32::from_le_bytes(wait_cmd_response(cmd_response_channel)?);
            blocks_checked += 1;
            if let Some(info) = outstanding_blocks.pop_front() {
                if blocks_checked % 10 == 0 {
                    println!("CRC checked {} of {} blocks", blocks_checked, total_blocks,);
                }
                if crc != info.desired_crc {
                    need_to_write.push(info);
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
