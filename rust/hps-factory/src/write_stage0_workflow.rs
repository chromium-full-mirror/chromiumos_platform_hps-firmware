// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::WriteStage0Config;
use crate::STAGE0_BYTES;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use hps_interface::Stage;
use mcu_common::commands::Command;
use mcu_common::FLASH_START;
use stm32_bootloader_client::MAX_READ_WRITE_SIZE;

const ROW_SIZE: usize = 256;

pub(crate) fn run<I, E>(
    i2c: &mut I,
    config: &Config,
    write_stage0_config: &WriteStage0Config,
) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config.clone());
    if let Ok(mut hps) = i2c.open_hps() {
        hps.perform_command(Command::Reset)?;
        if let Ok(mut hps) = i2c.open_hps() {
            match hps.stage()? {
                Stage::Stage0 => {
                    if write_stage0_config.existing_stage0_ok {
                        println!("Stage0 already present on device");
                        return Ok(());
                    } else {
                        bail!("Stage0 already present and needs to be erased before update");
                    }
                }
                other => bail!("HPS firmware is present, but found {:?} not stage0", other),
            }
        }
        // We've probably back to the system bootloader, fall through with
        // regular flashing procedure.
    }
    let mut stm32 = i2c.open_bootloader()?;

    println!("Erasing MCU flash");
    stm32.erase_flash()?;

    let mut stage0_bytes = STAGE0_BYTES.to_vec();
    // Pad out program out to an exact number of flash rows. We pad with 0xff
    // because that's what empty flash contains. Writing e.g. 100 bytes results
    // in the bootloader rejecting the write. Possibly it's using fast writes,
    // which are whole rows.
    stage0_bytes.resize((stage0_bytes.len() - 1) / ROW_SIZE * ROW_SIZE + 1, 0xff);

    // We write backwards rather than fowards and verify as we go. If we wrote
    // forwards, then a failure after we've written to the start of flash would
    // result in the system bootloader not starting next time we boot. This
    // could result in a bricked device that would require physical access to
    // the debug pins to unbrick.
    let mut bar = progress::Bar::new();
    bar.set_job_title("Writing stage0");
    let mut complete = 0;
    let mut read_back = [0u8; MAX_READ_WRITE_SIZE];
    for (chunk_number, chunk) in stage0_bytes.chunks(MAX_READ_WRITE_SIZE).enumerate().rev() {
        let address = FLASH_START + (MAX_READ_WRITE_SIZE * chunk_number) as u32;
        stm32.write_memory(address, chunk).with_context(|| {
            format!(
                "Failed to write {} bytes at address 0x{:x}",
                chunk.len(),
                address
            )
        })?;
        stm32.read_memory(address, &mut read_back[..chunk.len()])?;
        if &read_back[..chunk.len()] != chunk {
            bail!("Verification failed")
        }
        complete += chunk.len();
        bar.reach_percent((complete * 100 / stage0_bytes.len()) as i32);
    }
    bar.reach_percent(100);
    println!();
    println!("Stage0 written and verified");

    Ok(())
}
