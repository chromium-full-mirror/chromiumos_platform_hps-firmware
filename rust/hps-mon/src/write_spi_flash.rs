// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::debug_commands;
use crate::debug_commands::ConsoleOutput;
use crate::DataSink;
use crate::DataSource;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use crc::Crc;
use fpga_app::camera::IMAGE_HEIGHT;
use fpga_app::camera::IMAGE_WIDTH;
use hps_interface::Hps;
use mcu_common::memory_banks;
use mcu_common::McuDebugCommand;
use mcu_common::SPI_FLASH_SIZE;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

const PAGE_SIZE: usize = 256;
const BLOCK_SIZE: usize = 64 * 1024;
const PAGES_PER_BLOCK: usize = BLOCK_SIZE / PAGE_SIZE;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);
const MAX_OUTSTANDING_COMMANDS: usize = 28;

pub(crate) fn write_file_to_spi_flash(
    command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    hps: Option<Arc<Mutex<dyn Hps + Send>>>,
    start_offset: usize,
    spi_flash_file: &Path,
    console_output: &ConsoleOutput,
) -> Result<()> {
    write_spi_flash(
        command_channel,
        cmd_response_channel,
        hps,
        start_offset,
        std::fs::read(spi_flash_file)?,
        console_output,
    )
}

fn write_spi_flash(
    command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    hps: Option<Arc<Mutex<dyn Hps + Send>>>,
    start_offset: usize,
    mut file_bytes: Vec<u8>,
    console_output: &ConsoleOutput,
) -> Result<()> {
    assert!(start_offset % BLOCK_SIZE == 0);
    let mut hps_writer = hps.map(HpsWriter::new);
    if let (Some(command_channel), Some(cmd_response_channel)) =
        (command_channel.as_mut(), cmd_response_channel.as_mut())
    {
        if file_bytes.len() > SPI_FLASH_SIZE as usize {
            bail!(
                "File is {:0.1}MB, which is too big to fit in the SPI flash ({}MB)",
                file_bytes.len() as f64 / 1024.0 / 1024.0,
                SPI_FLASH_SIZE / 1024 / 1024
            );
        }
        if file_bytes.len() + start_offset > SPI_FLASH_SIZE as usize {
            bail!(
                "File is small enough to fit in SPI flash, but not when placed at offset 0x{:x}",
                start_offset
            );
        }

        // Pad with 0xff (blank flash) up to end of current block boundary.
        file_bytes.resize(
            (file_bytes.len() + BLOCK_SIZE - 1) & !(BLOCK_SIZE - 1),
            0xff,
        );

        let blocks_to_write = get_blocks_to_write(
            &file_bytes,
            command_channel,
            start_offset / BLOCK_SIZE,
            cmd_response_channel,
            console_output,
        )?;
        if blocks_to_write.is_empty() {
            console_output.print("All blocks already up-to-date. Nothing to write.\n");
            return Ok(());
        }
        let total_blocks = file_bytes.len() / BLOCK_SIZE;
        console_output.print(format!(
            "Need to write {} of {} blocks\n",
            blocks_to_write.len(),
            total_blocks
        ));

        let start = std::time::Instant::now();
        let mut pages_written = 0;
        let mut i2c_pages = 0;
        let mut outstanding_commands = 0;
        let total_pages = blocks_to_write.len() * PAGES_PER_BLOCK;
        for block_info in blocks_to_write {
            for page_offset in 0..PAGES_PER_BLOCK {
                let page_num = block_info.block_num * PAGES_PER_BLOCK + page_offset;
                let chunk =
                    &block_info.bytes[page_offset * PAGE_SIZE..(page_offset + 1) * PAGE_SIZE];

                let mut written_via_i2c = false;
                let page_within_block = page_num % PAGES_PER_BLOCK;
                if page_within_block > 0 && chunk == [0xff; PAGE_SIZE] {
                    // Page is blank and not the first page of the block. Since
                    // all pages were erased when the first page of the block
                    // was written, we don't need to write it with blank data.
                    continue;
                }
                if let Some(hps) = hps_writer.as_mut() {
                    // Blocks get erased when we write the first page of the
                    // block. We need to avoid the situation where we send a
                    // page of data over I2C that is mid-way through a block
                    // that hasn't yet been erased. We take the somewhat crude
                    // approach of only allowing I2C writes to parts of the
                    // block beyond MAX_OUTSTANDING_COMMANDS pages. That way the
                    // first page of the block is guaranteed to have been
                    // processed by the time we do the I2C write.
                    if page_within_block > MAX_OUTSTANDING_COMMANDS
                        && hps.try_write((page_num * PAGE_SIZE) as u32, chunk)
                    {
                        written_via_i2c = true;
                        i2c_pages += 1;
                    }
                }

                if !written_via_i2c {
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
                }
                pages_written += 1;
                if pages_written % 100 == 0 {
                    console_output.print(format!(
                        "{:0.1}% of {}KiB complete. {:0.1}KiB/s\n",
                        pages_written as f64 * 100f64 / total_pages as f64,
                        total_pages * PAGE_SIZE / 1024,
                        ((pages_written * PAGE_SIZE) as f64 / start.elapsed().as_secs_f64())
                            / 1024f64
                    ));
                }
            }
        }

        console_output.print(format!(
            "Finished sending data, waiting for last {} writes to complete\n",
            outstanding_commands
        ));

        if let Some(hps) = hps_writer.take() {
            hps.wait_done()?;
        }

        for _ in 0..outstanding_commands {
            wait_cmd_response(cmd_response_channel)?;
        }

        let seconds = start.elapsed().as_secs_f64();
        console_output.print(format!(
            "Wrote {} pages to SPI flash in {:0.1} seconds ({:0.1}KiB/s). {} pages via I2C\n",
            pages_written,
            seconds,
            ((total_pages * PAGE_SIZE) as f64 / seconds) / 1024f64,
            i2c_pages,
        ));
    } else {
        bail!("No command channel available, command not sent.");
    }
    Ok(())
}

pub(crate) fn write_test_data(
    command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    hps: Option<Arc<Mutex<dyn Hps + Send>>>,
    test_data_dir: &Path,
    console_output: &ConsoleOutput,
) -> Result<()> {
    let mut address = fpga_app::TEST_IMAGES_OFFSET as usize;
    let mut filenames = Vec::new();
    for entry in test_data_dir.read_dir()? {
        let entry = entry?;
        if let Some(extension) = entry.path().extension() {
            if extension == "png" {
                filenames.push(entry.path().to_owned());
            }
        }
    }
    filenames.sort();

    for path in &filenames {
        console_output.print(format!("Writing test image {}\n", path.display()));
        if address as u32 >= fpga_app::TEST_IMAGES_OFFSET + fpga_app::TEST_IMAGES_MAX_SIZE {
            bail!("Test image area is full");
        }
        let image = image::open(path)
            .map_err(|e| anyhow!("Failed to process {}: {e}", path.display()))?
            .into_luma8();
        if image.width() != IMAGE_WIDTH || image.height() != IMAGE_HEIGHT {
            bail!(
                "Image {} has unexpected dimensions {}x{}",
                path.display(),
                image.width(),
                image.height()
            );
        }
        write_spi_flash(
            command_channel,
            cmd_response_channel,
            hps.clone(),
            address,
            image.into_raw(),
            console_output,
        )?;
        address += fpga_app::TEST_IMAGE_BLOCK_SIZE;
    }
    Ok(())
}

/// Write data to the HPS over I2C.
struct HpsWriter {
    join_handle: JoinHandle<Result<()>>,
    sender: mpsc::SyncSender<BlockToWrite>,
}

impl HpsWriter {
    fn new(hps: Arc<Mutex<dyn Hps + Send>>) -> Self {
        // I2C via MCP2221 is really slow on account of USB protocol that it
        // uses. It ends up not transmitting data continuously. So we don't want
        // to put too much in the queue, otherwise we'll end up having to wait a
        // while at the end for the queue to clear. However we want a large
        // enough queue that we can keep the I2C bus busy while RTT is writing
        // the first MAX_OUTSTANDING_COMMANDS pages of each block.
        // Experimentally, 4 seems to give reasonable results.
        const QUEUE_SIZE: usize = 4;
        let (sender, receiver) = mpsc::sync_channel::<BlockToWrite>(QUEUE_SIZE);
        let join_handle = std::thread::spawn(move || -> Result<()> {
            let mut hps = hps.lock().unwrap();
            let bank = memory_banks::SPI_FLASH;

            while let Ok(work) = receiver.recv() {
                hps.write_memory(bank, work.address, &work.bytes)?;
                hps.wait_memory_bank_available(bank)?;
            }
            Ok(())
        });
        Self {
            join_handle,
            sender,
        }
    }

    /// Tries to queue writing of `bytes` at `address`. Returns false if the
    /// queue is already full.
    fn try_write(&mut self, address: u32, bytes: &[u8]) -> bool {
        self.sender
            .try_send(BlockToWrite {
                bytes: bytes.to_owned(),
                address,
            })
            .is_ok()
    }

    /// Blocks until any outstanding writes are complete.
    fn wait_done(self) -> Result<()> {
        // Drop sender to let thread know that we're done.
        drop(self.sender);
        self.join_handle
            .join()
            .map_err(|_| anyhow!("Failed to join thread"))??;
        Ok(())
    }
}

struct BlockToWrite {
    bytes: Vec<u8>,
    address: u32,
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
    console_output: &ConsoleOutput,
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
                if blocks_checked % 10 == 0 || blocks_checked == total_blocks {
                    console_output.print(format!(
                        "CRC checked {blocks_checked} of {total_blocks} blocks\n"
                    ));
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
