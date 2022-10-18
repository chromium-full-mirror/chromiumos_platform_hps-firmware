// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use hps_interface::Stage;
use log::info;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::registers::Register;
use mcu_common::Error;

pub(crate) struct Stage1Tester<'a, I: Send> {
    i2c: HpsI2c<'a, I>,
}

impl<'a, I, E> Stage1Tester<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn run(i2c: &mut I, global_config: &Config) -> Result<()> {
        let i2c = HpsI2c::new(i2c, global_config.clone());
        let mut tester = Stage1Tester { i2c };

        tester.test_panic_handling();
        tester.test_read_invalid_register();
        tester.test_write_invalid_register();
        tester.test_read_with_invalid_first_byte();
        tester.test_write_with_invalid_first_byte();
        tester.test_unsupported_command();
        tester.test_write_beyond_end_of_spi_flash();

        info!("Stage1 test passed");
        Ok(())
    }

    fn test_panic_handling(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        assert_eq!(hps.stage().unwrap(), Stage::Stage1);
        hps.check_errors().unwrap();
        hps.perform_command(Command::TriggerMcuPanic).unwrap();
        Self::ensure_stage1(&mut hps).unwrap();
        assert_eq!(hps.error().unwrap(), Error::Panic);
        let report = hps.crash_report().unwrap();
        assert!(report.contains("Intentional panic"));
        drop(hps);
        let mut hps = self.reset_to_stage1().unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
    }

    fn test_read_invalid_register(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        let command = [0x79 | 0x80]; // non-existent register 0x79
        let response = hps.write_read_unchecked(&command, 64).unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
        assert_eq!(response, [0; 64]);
    }

    fn test_write_invalid_register(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        let bytes = [
            0x79 | 0x80, // non-existent register 0x79
            10,
        ];
        hps.write_unchecked(&bytes).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_read_with_invalid_first_byte(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        // First byte 0b01xxxxxx is neither a register access nor a memory bank access.
        let command = [0b01000000];
        let response = hps.write_read_unchecked(&command, 64).unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
        assert_eq!(response, [0; 64]);
    }

    fn test_write_with_invalid_first_byte(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        // First byte 0b01xxxxxx is neither a register access nor a memory bank access.
        let bytes = [0b01000000, 0x12, 0x34, 0x56, 0x78];
        hps.write_unchecked(&bytes).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_unsupported_command(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        hps.perform_command(Command::Launch1).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
        drop(hps);

        let mut hps = self.reset_to_stage1().unwrap();
        hps.write_register(Register::Command, 255).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_write_beyond_end_of_spi_flash(&mut self) {
        let mut hps = self.reset_to_stage1().unwrap();
        hps.write_memory(memory_banks::SPI_FLASH, 16 * 1024 * 1024, &[0u8; 16])
            .unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    /// Resets the HPS, launches to stage1 then waits for it start.
    fn reset_to_stage1(&mut self) -> Result<HpsAdapter> {
        let mut hps = self.i2c.open_hps()?;
        hps.perform_command(Command::Reset)?;
        Self::ensure_stage1(&mut hps)?;
        Ok(hps)
    }

    /// Launches stage1 and waits for it to start. If stage1 is already running,
    /// does nothing.
    fn ensure_stage1(hps: &mut HpsAdapter) -> Result<()> {
        hps.wait_ready()?;
        if hps.stage()? == Stage::Stage0 {
            hps.perform_command(Command::Launch1)?;
        }
        hps.wait_ready()?;
        assert_eq!(hps.stage().unwrap(), Stage::Stage1);
        Ok(())
    }
}
