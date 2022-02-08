// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::APPLICATION_BYTES;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use log::info;
use mcu_common::commands::Command;
use mcu_common::Status;

pub(crate) fn run<I, E>(i2c: &mut I, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config);

    if let Ok(mut stm32) = i2c.open_bootloader() {
        // If the system bootloader is already running, then most likely the
        // flash is empty, but just to be sure, we erase it.
        info!("Erasing flash using STM32 bootloader");
        stm32.erase_flash()?;
        return Ok(());
    }

    // Erasing is done by stage1_app. Write an up-to-date dev version of
    // stage1_app so that we know it supports the required commands.
    i2c.write_stage1(APPLICATION_BYTES)?;
    i2c.execute_stage1().context("Failed to execute stage1")?;

    let mut hps = i2c.open_hps()?;
    if hps.status()?.contains(Status::STAGE0_PERM_LOCKED) {
        bail!("Stage0 is permanently write-protected and cannot be erased");
    }
    hps.perform_command(Command::EraseStage0Start)?;
    Ok(())
}
