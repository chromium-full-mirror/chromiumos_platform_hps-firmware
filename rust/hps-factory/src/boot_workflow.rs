// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;

use crate::hps_i2c::HpsI2c;
use crate::Config;

pub(crate) fn run<I, E>(i2c: &mut I, boot_config: &crate::BootConfig, config: &Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config.clone());
    i2c.write_stage1(&config.mcu_rom()?)?;
    i2c.execute_stage1().context("Execute application")?;
    if !boot_config.skip_bitstream_write {
        i2c.write_gateware().context("Write gateware")?;
    }
    i2c.write_soc_rom().context("Write SOC ROM")?;
    i2c.execute_application().context("Execute application")?;
    Ok(())
}
