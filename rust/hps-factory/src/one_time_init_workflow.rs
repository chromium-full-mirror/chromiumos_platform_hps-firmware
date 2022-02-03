// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c;
use hps_interface::Hps;
use hps_interface::Stage;
use log::info;
use mcu_common::registers;
use mcu_common::OptionBytesConfigRequest;

const ONE_TIME_INIT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init.bin"));

pub(crate) fn run<I, E>(i2c: &mut I, config: Config) -> Result<()>
where
    I: i2c::Read<Error = E> + i2c::Write<Error = E> + i2c::WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config);
    i2c.write_stage1(ONE_TIME_INIT_BYTES)?;
    i2c.execute_non_i2c_stage1()
        .context("Failed to launch one-time-init binary")?;

    let mut hps = i2c
        .open_hps()
        .context("One-time-init binary did not start")?;
    let stage = hps.stage()?;
    if stage != Stage::OneTimeInit {
        bail!(
            "Expected to be running one-time-init stage, but found: {:?}",
            stage
        );
    }
    let options = OptionBytesConfigRequest::RESET_PIN | OptionBytesConfigRequest::RELOAD;
    hps.write_register(registers::CONFIGURE_OPTION_BYTES, options.bits())
        .context("Failed to configure option bytes")?;
    // The ConfigureOptionBytes command, assuming it succeeded, will have reset
    // the device, so we need to reopen it.
    let mut hps = i2c
        .open_hps()
        .context("HPS lost after updating option bytes")?;
    hps.check_errors()?;
    let stage = hps.stage()?;
    if stage != Stage::Stage0 {
        bail!("Expected to reset to Stage0, but found {:?}", stage);
    }

    info!("One-time-init complete");
    Ok(())
}
