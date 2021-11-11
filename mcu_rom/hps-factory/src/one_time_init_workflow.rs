// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::bail;
use anyhow::Result;
use embedded_hal::blocking::i2c;
use hps_interface::Hps;
use mcu_common::commands::Command;

const ONE_TIME_INIT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init.bin"));

pub(crate) fn run<I, E>(i2c: &mut I, config: Config) -> Result<()>
where
    I: i2c::Read<Error = E> + i2c::Write<Error = E> + i2c::WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config);
    i2c.write_stage1(ONE_TIME_INIT_BYTES)?;
    let mut hps = i2c.open_hps()?;
    hps.perform_command(Command::Reset)?;
    let mut hps = i2c.open_hps()?;
    hps.perform_command(Command::Launch1)?;

    if i2c.open_bootloader().is_ok() || i2c.open_hps().is_ok() {
        println!("One-time-init complete");
    } else {
        bail!("HPS reset after one-time-init failed");
    }
    Ok(())
}
