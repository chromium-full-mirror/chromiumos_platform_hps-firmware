// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::debug_commands;
use crate::DataSink;
use crate::DataSource;
use anyhow::bail;
use anyhow::Result;
use mcu_common::McuDebugCommand;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn write_spi_flash(
    command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    spi_flash_file: &Option<PathBuf>,
) -> Result<()> {
    let spi_flash_file = match spi_flash_file {
        Some(x) => x,
        None => bail!("--spi-flash-file must be specified on startup"),
    };
    if let (Some(command_channel), Some(cmd_response_channel)) =
        (command_channel.as_mut(), cmd_response_channel.as_mut())
    {
        let mut file_bytes = std::fs::read(spi_flash_file)?;
        const PAGE_SIZE: usize = 256;

        // Pad with 0 up to end of current page boundary.
        file_bytes.resize((file_bytes.len() + PAGE_SIZE - 1) & !(PAGE_SIZE - 1), 0);

        let start = std::time::Instant::now();
        let mut pages_written = 0;
        let mut outstanding_commands = 0;
        let total_pages = file_bytes.len() / PAGE_SIZE;
        for (page_num, chunk) in file_bytes.chunks(PAGE_SIZE).enumerate() {
            let command = debug_commands::DebugCommand {
                code: McuDebugCommand::WriteSpiFlash.into(),
                arg: page_num as u16,
            };
            command_channel.write(&command.to_bytes())?;
            command_channel.write(chunk)?;
            outstanding_commands += 1;
            if outstanding_commands > 15 {
                wait_cmd_response(cmd_response_channel)?;
                outstanding_commands -= 1;
            }
            pages_written += 1;
            if pages_written % 10 == 0 {
                println!(
                    "{:0.1}% complete. {:0.1}KiB/s",
                    pages_written as f64 * 100f64 / total_pages as f64,
                    ((pages_written * PAGE_SIZE) as f64 / start.elapsed().as_secs_f64()) / 1024f64
                );
            }
        }

        for _ in 0..outstanding_commands {
            wait_cmd_response(cmd_response_channel)?;
        }

        let seconds = start.elapsed().as_secs_f64();
        println!(
            "Wrote {} pages to SPI flash in {:0.1} seconds ({:0.1}KiB/s)",
            pages_written,
            seconds,
            (file_bytes.len() as f64 / seconds) / 1024f64
        );
    } else {
        bail!("No command channel available, command not sent.");
    }
    Ok(())
}

fn wait_cmd_response(cmd_response_channel: &mut Box<dyn DataSource>) -> Result<()> {
    // If we're using OpenOCD, then reads on this channel should have been set
    // to block. If we're using an ST-Link, then we'll effectively end up
    // polling. In either case, polling will occur, with OpenOCD doing the
    // polling if it's being used.
    let mut remaining_bytes = 4;
    loop {
        remaining_bytes -= cmd_response_channel.read(&mut [0u8; 4])?;
        if remaining_bytes == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
