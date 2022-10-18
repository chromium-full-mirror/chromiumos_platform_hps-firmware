// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use log::info;
use mcu_common::registers::Register;
use mcu_common::OptionBytesConfigRequest;
use mcu_common::Status;
use std::fmt::Display;

#[derive(Debug)]
pub(crate) enum EraseError {
    PermanentlyLocked,
    Other(anyhow::Error),
}

pub(crate) fn run<I, E>(i2c: &mut I, config: Config) -> Result<(), EraseError>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config);

    if let Ok(mut stm32) = i2c.open_bootloader() {
        // If the system bootloader is already running, then most likely the
        // flash is empty, but just to be sure, we erase it.
        info!("Erasing flash using STM32 bootloader");
        stm32
            .erase_flash()
            .map_err(|e| -> anyhow::Error { e.into() })?;
        return Ok(());
    }

    let stage0_version = i2c.open_hps()?.read_register(Register::HardwareVersion)?;
    let one_time_init_bytes = if stage0_version == 0x0101 {
        info!("Old version of stage0. Loading legacy one_time_init_loader.");
        crate::firmware::ONE_TIME_INIT_LOADER_LEGACY_BYTES
    } else {
        crate::firmware::ONE_TIME_INIT_LOADER_BYTES
    };

    if i2c.open_hps()?.status()?.contains(Status::WPON) {
        return Err(EraseError::Other(anyhow!(
            "Erasing stage0 requires that firmware write protect is off"
        )));
    }

    // Erasing is done by one_time_init. Write the loader that will put it into
    // RAM and run it.
    if let Err(error) = i2c.write_stage1(one_time_init_bytes) {
        // We got an error writing stage1. This might be because we're running
        // an old version of stage0 that doesn't support the EraseStage1
        // command.
        if i2c.open_hps()?.error()? == mcu_common::Error::HostI2cBadRequest {
            info!("Stage0 reported a bad request. Retrying without EraseStage1");
            i2c.reset_to_bootloader()?;
            // Try again, without using the erase command.
            i2c.erase_stage1_allowed = false;
            i2c.write_stage1(one_time_init_bytes)?;
        } else {
            return Err(error.into());
        }
    }
    i2c.execute_non_i2c_stage1()
        .context("Failed to execute one_time_init")?;

    let current_options = OptionBytesConfigRequest::from_bits_truncate({
        let mut hps = i2c.open_hps()?;
        hps.check_errors()?;
        hps.read_register(Register::ConfigurationOptionBytes)?
    });
    if current_options.contains(OptionBytesConfigRequest::RDP2) {
        info!("Stage0 is permanently locked and cannot be erased.");
        return Err(EraseError::PermanentlyLocked);
    }
    if current_options.contains(OptionBytesConfigRequest::WRITE_PROTECT)
        && !current_options.contains(OptionBytesConfigRequest::RDP1)
    {
        // If write protect has been applied to stage0, but we're still on RDP0,
        // then switch temporarily to RDP1. This avoids the need to implement a
        // special remove-write-protection command in one_time_init.
        info!("Stage0 is write protected with no RDP. Temporarily applying RDP1.");
        i2c.open_hps()?.write_register(
            Register::ConfigurationOptionBytes,
            (OptionBytesConfigRequest::RDP1 | OptionBytesConfigRequest::RELOAD).bits(),
        )?;
        i2c.execute_non_i2c_stage1()
            .context("Failed to restart one_time_init after temporary RDP1 setting")?;
    }
    i2c.open_hps()?.write_register(
        Register::ConfigurationOptionBytes,
        (OptionBytesConfigRequest::ERASE | OptionBytesConfigRequest::RELOAD).bits(),
    )?;
    i2c.open_bootloader()
        .context("System bootloader didn't respond after erasing stage0")?;
    Ok(())
}

impl From<anyhow::Error> for EraseError {
    fn from(other: anyhow::Error) -> Self {
        EraseError::Other(other)
    }
}

impl std::error::Error for EraseError {}

impl Display for EraseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EraseError::PermanentlyLocked => {
                write!(
                    f,
                    "Stage0 is permanently write-protected and cannot be erased"
                )
            }
            EraseError::Other(other) => write!(f, "{}", other),
        }
    }
}
