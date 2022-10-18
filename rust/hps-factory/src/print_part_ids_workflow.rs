// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;

pub(crate) fn run<I, E>(i2c: &mut I, config: &Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config.clone());
    i2c.write_firmware()?;
    i2c.execute_application().context("Execute application")?;

    let mut hps = i2c.open_hps()?;

    println!("{}", hps.read_part_ids()?);
    Ok(())
}
