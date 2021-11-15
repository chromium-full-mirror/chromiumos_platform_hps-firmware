// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::bail;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use hps_interface::Stage;
use mcu_common::commands::Command;

pub(crate) fn run<I, E>(i2c: &mut I, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config);
    let mut hps = i2c.open_hps()?;
    let stage = hps.stage()?;
    // If we're not in stage0, then reset
    if stage != Stage::Stage0 {
        hps.perform_command(Command::Reset)?;
    }
    hps = i2c.open_hps()?;
    if hps.stage()? != Stage::Stage0 {
        bail!("Failed to reset into stage0")
    }
    hps.perform_command(Command::EraseStage0Start)?;
    println!("Stage0 erased. Please power-cycle the HPS to return to the system bootloader");
    Ok(())
}
