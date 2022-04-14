// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::BorrowedI2c;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use hps_interface::Stage;
use log::info;
use mcu_common::commands::Command;
use mcu_common::Error;

pub(crate) struct Stage1Tester<'a, I> {
    i2c: HpsI2c<'a, I>,
}

impl<'a, I, E> Stage1Tester<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn run(i2c: &mut I, global_config: &Config) -> Result<()> {
        let i2c = HpsI2c::new(i2c, global_config.clone());
        let mut tester = Stage1Tester { i2c };

        tester.test_panic_handling();

        info!("Stage1 test passed");
        Ok(())
    }

    fn test_panic_handling(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        assert_eq!(hps.stage().unwrap(), Stage::Stage1);
        hps.check_errors().unwrap();
        hps.perform_command(Command::TriggerMcuPanic).unwrap();
        hps = self.reset_to_stage1().unwrap();
        assert_eq!(hps.error().unwrap(), Error::Panic);
        let report = hps.crash_report().unwrap();
        assert!(report.contains("Intentional panic"));
        hps = self.reset_to_stage1().unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
    }

    /// Resets the HPS, launches to stage1 then waits for it start
    fn reset_to_stage1(&mut self) -> Result<HpsAdapter<BorrowedI2c<I>>> {
        self.i2c
            .open_hps()
            .unwrap()
            .perform_command(Command::Reset)
            .unwrap();
        self.i2c.open_hps()?.perform_command(Command::Launch1)?;
        let mut hps = self.i2c.open_hps()?;
        assert_eq!(hps.stage().unwrap(), Stage::Stage1);
        Ok(hps)
    }
}
