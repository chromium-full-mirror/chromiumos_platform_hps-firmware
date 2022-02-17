// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::WriteStage0Config;
use crate::ONE_TIME_INIT_LOADER_BYTES;
use crate::STAGE0_BYTES;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use hps_interface::Stage;
use indicatif::ProgressBar;
use log::info;
use mcu_common::commands::Command;
use mcu_common::registers;
use mcu_common::OptionBytesConfigRequest;
use mcu_common::APPLICATION_START_ADDRESS;
use mcu_common::APPLICATION_VECTOR_TABLE_ADDRESS;
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
                        info!("Stage0 already present on device");
                        return Ok(());
                    } else if write_stage0_config.update {
                        info!("Stage0 already present on device, erasing.");
                        crate::erase_stage0_workflow::run(i2c.i2c, config.clone())?;
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

    info!("Erasing MCU flash");
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
    info!("Writing stage0");
    let bar = ProgressBar::new(stage0_bytes.len() as u64);
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
        bar.inc(chunk.len() as u64);
    }
    bar.finish();
    info!("Stage0 written and verified");

    i2c.write(
        "one-time-init",
        APPLICATION_START_ADDRESS,
        ONE_TIME_INIT_LOADER_BYTES,
    )?;

    // Launch the one-time-init program. It will detect that it was launched
    // from the system bootloader and will reset into stage0.
    i2c.open_bootloader()?
        .go(APPLICATION_VECTOR_TABLE_ADDRESS)?;

    // Tell stage0 to launch one-time-init again. Note, we don't use
    // execute_non_i2c_stage1 because it will reset stage0, which won't work on
    // proto2 dev boards where OptionBytesConfigRequest::RESET_PIN is not yet
    // applied. We don't need a reset because one_time_init will already have
    // reset via standby, which is not affected by the reset pin not already
    // having been configured.
    let mut hps = i2c.open_hps()?;
    hps.perform_command(Command::Launch1)?;
    let mut hps = i2c.open_hps()?;
    let stage = hps.stage()?;
    if stage != Stage::OneTimeInit {
        bail!(
            "Expected to be running one-time-init stage, but found: {:?}",
            stage
        );
    }
    info!("Configuring option bytes");
    let mut options = OptionBytesConfigRequest::RESET_PIN | OptionBytesConfigRequest::RELOAD;
    if write_stage0_config.disable_boot0 {
        options |= OptionBytesConfigRequest::DISABLE_BOOT0_PIN;
    }
    if write_stage0_config.permanent_lock {
        options |= OptionBytesConfigRequest::PERMANENT_WRITE_PROTECT;
    } else {
        options |= OptionBytesConfigRequest::NON_PERMANENT_WRITE_PROTECT;
    }

    hps.write_register(registers::CONFIGURE_OPTION_BYTES, options.bits())
        .context("Failed to configure option bytes")?;
    // The configure option bytes request, assuming it succeeded, will have
    // reset the device, so we need to reopen it.
    let mut hps = i2c
        .open_hps()
        .context("HPS lost after updating option bytes")?;
    hps.check_errors()?;
    let stage = hps.stage()?;
    if stage != Stage::Stage0 {
        bail!("Expected to reset to Stage0, but found {:?}", stage);
    }
    i2c.execute_non_i2c_stage1()?;
    let mut hps = i2c.open_hps()?;
    let applied_options =
        OptionBytesConfigRequest::from_bits(hps.read_register(registers::CONFIGURE_OPTION_BYTES)?)
            .ok_or_else(|| anyhow!("Got invalid bits when reading applied options"))?;
    options.remove(OptionBytesConfigRequest::RELOAD);
    if applied_options != options {
        bail!(
            "Options didn't apply correctly. Expected [{:?}], got [{:?}]",
            options,
            applied_options
        );
    }

    info!("Stage0 written and non-permanently locked");

    Ok(())
}
