// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::STAGE0_BYTES;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use mcu_common::FLASH_START;
use stm32_bootloader_client::MAX_READ_WRITE_SIZE;

pub(crate) fn run<I, E>(i2c: &mut I, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config);
    let mut stm32 = i2c.open_bootloader()?;

    println!("Erasing MCU flash");
    stm32.erase_flash()?;

    // We write backwards rather than fowards and verify as we go. If we wrote
    // forwards, then a failure after we've written to the start of flash would
    // result in the system bootloader not starting next time we boot. This
    // could result in a bricked device that would require physical access to
    // the debug pins to unbrick.
    let mut bar = progress::Bar::new();
    bar.set_job_title("Writing stage0");
    let mut complete = 0;
    let mut read_back = [0u8; MAX_READ_WRITE_SIZE];
    for (chunk_number, chunk) in STAGE0_BYTES.chunks(MAX_READ_WRITE_SIZE).enumerate().rev() {
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
        bar.reach_percent((complete * 100 / STAGE0_BYTES.len()) as i32);
    }
    bar.reach_percent(100);
    println!();
    println!("Stage0 written and verified");

    Ok(())
}
