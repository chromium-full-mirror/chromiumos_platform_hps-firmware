// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::ONE_TIME_INIT_LOADER_BYTES;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use log::info;
use mcu_common::registers;
use mcu_common::OptionBytesConfigRequest;

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

    // Erasing is done by one_time_init. Write the loader that will put it into
    // RAM and run it.
    i2c.write_stage1(ONE_TIME_INIT_LOADER_BYTES)?;
    i2c.execute_non_i2c_stage1()
        .context("Failed to execute one_time_init")?;

    let mut hps = i2c.open_hps()?;
    let current_options = OptionBytesConfigRequest::from_bits_truncate(
        hps.read_register(registers::CONFIGURE_OPTION_BYTES)?,
    );
    if current_options.contains(OptionBytesConfigRequest::PERMANENT_WRITE_PROTECT) {
        bail!("Stage0 is permanently write-protected and cannot be erased");
    }
    hps.write_register(
        registers::CONFIGURE_OPTION_BYTES,
        (OptionBytesConfigRequest::ERASE | OptionBytesConfigRequest::RELOAD).bits(),
    )?;
    Ok(())
}
