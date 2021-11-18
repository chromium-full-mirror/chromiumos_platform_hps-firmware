// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::BootLoopConfig;
use crate::Config;
use crate::APPLICATION_BYTES;
use crate::STAGE0_SYS_BYTES;
use anyhow::Result;
use embedded_hal::blocking::i2c;
use hps_interface::Hps;
use mcu_common::commands::Command;

const STAGE0_ADDRESS: u32 = 0x08000400;

pub(crate) fn run<I, E>(
    i2c: &mut I,
    config: &Config,
    boot_loop_config: &BootLoopConfig,
) -> Result<()>
where
    I: i2c::Read<Error = E> + i2c::Write<Error = E> + i2c::WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config.clone());
    if !boot_loop_config.skip_firmware_writes {
        i2c.open_bootloader()?.erase_flash()?;
        i2c.write("stage0", STAGE0_ADDRESS, STAGE0_SYS_BYTES)?;
        i2c.write_stage1(APPLICATION_BYTES)?;
    }
    let mut cycle = 0;
    loop {
        println!("Cycle {}", cycle);

        println!("System bootloader -> stage0");
        i2c.open_bootloader()?.go(STAGE0_ADDRESS)?;

        println!("stage0 -> stage1");
        i2c.execute_stage1()?;

        println!("Resetting to system bootloader");
        i2c.open_hps()?.perform_command(Command::Reset)?;

        cycle += 1;
    }
}
