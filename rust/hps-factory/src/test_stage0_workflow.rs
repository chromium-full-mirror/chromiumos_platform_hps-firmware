// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::time::Instant;

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::bail;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use log::info;
use mcu_common::commands::Command;
use mcu_common::registers::Register;
use mcu_common::Error;
use mcu_common::Status;
use mcu_common::FLASH_WRITE_SZ;
use mcu_common::STAGE1_SLOT_LENGTH;

pub(crate) struct Stage0Tester<'a, I: Send> {
    i2c: HpsI2c<'a, I>,
}

impl<'a, I, E> Stage0Tester<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn run(i2c: &mut I, global_config: &Config) -> Result<()> {
        let i2c = HpsI2c::new(i2c, global_config.clone());
        let mut tester = Stage0Tester { i2c };
        let mut hps = tester.reset_hps()?;
        let stage0_version = hps.stage0_version()?;
        info!("Starting stage0 test with version {}", stage0_version);
        if stage0_version < 4 {
            bail!("This workflow requires stage0 version 4 or greater");
        }
        if hps.status()?.contains(Status::WPOFF) {
            bail!("This test needs firmware write-protect on");
        }
        drop(hps);

        tester.test_erase_stage1();
        tester.test_write_invalid_offset();
        // Note, test_write_to_end assumes flash is blank, so needs to be after
        // test_erase_stage1.
        tester.test_write_to_end();
        tester.test_read_invalid_register();
        tester.test_write_invalid_register();
        tester.test_read_with_invalid_first_byte();
        tester.test_write_with_invalid_first_byte();
        tester.test_invalid_command();
        tester.test_unsupported_command();
        tester.test_load_dev_signed_stage1();

        info!("Stage0 test passed");
        Ok(())
    }

    fn test_write_invalid_offset(&mut self) {
        let data = vec![42; FLASH_WRITE_SZ * 2];

        // Try 1 double-word beyond the end of flash.
        let mut hps = self.reset_hps().unwrap();
        hps.write_memory(0, (STAGE1_SLOT_LENGTH + FLASH_WRITE_SZ) as u32, &data)
            .unwrap();
        hps.wait_ready().unwrap();
        assert_eq!(hps.error().unwrap(), Error::McuFlashWriteError);
        drop(hps);

        // Try 1 double-word less than u32::MAX in an attempt to get an integer
        // overflow when the address is added to the slot start address. Also
        // has the potential to trigger an overflow if len is added to address.
        let mut hps = self.reset_hps().unwrap();
        hps.write_memory(0, u32::MAX - (FLASH_WRITE_SZ as u32), &data)
            .unwrap();
        hps.wait_ready().unwrap();
        assert_eq!(hps.error().unwrap(), Error::McuFlashWriteError);
        drop(hps);

        // Try a write that starts in a valid location, but extends beyond the
        // end of flash.
        let mut hps = self.reset_hps().unwrap();
        hps.write_memory(0, (STAGE1_SLOT_LENGTH - FLASH_WRITE_SZ) as u32, &data)
            .unwrap();
        hps.wait_ready().unwrap();
        assert_eq!(hps.error().unwrap(), Error::McuFlashWriteError);
    }

    fn test_write_to_end(&mut self) {
        let data = vec![42; FLASH_WRITE_SZ * 2];

        // Try writing right up to the end of flash. This is a non-error
        // scenario, but one that we wouldn't generally exercise unless we were
        // getting incredibly low on MCU flash.
        let mut hps = self.reset_hps().unwrap();
        hps.write_memory(0, (STAGE1_SLOT_LENGTH - data.len()) as u32, &data)
            .unwrap();
        hps.wait_ready().unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
    }

    fn test_read_invalid_register(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        let command = [0x79 | 0x80]; // non-existent register 0x79
        let response = hps.write_read_unchecked(&command, 64).unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
        assert_eq!(response, [0; 64]);
    }

    fn test_write_invalid_register(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        let bytes = [
            0x79 | 0x80, // non-existent register 0x79
            0x00,
            0x0a,
        ];
        hps.write_unchecked(&bytes).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_read_with_invalid_first_byte(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        // First byte 0b01xxxxxx is neither a register access nor a memory bank access.
        let command = [0b01000000];
        let response = hps.write_read_unchecked(&command, 64).unwrap();
        assert_eq!(hps.error().unwrap(), Error::None);
        assert_eq!(response, [0; 64]);
    }

    fn test_write_with_invalid_first_byte(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        // First byte 0b01xxxxxx is neither a register access nor a memory bank access.
        let bytes = [0b01000000, 0x12, 0x34, 0x56, 0x78];
        hps.write_unchecked(&bytes).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_invalid_command(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        hps.write_register(Register::Command, 12345).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_unsupported_command(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        hps.perform_command(Command::LaunchApp).unwrap();
        assert_eq!(hps.error().unwrap(), Error::HostI2cBadRequest);
    }

    fn test_erase_stage1(&mut self) {
        let mut hps = self.reset_hps().unwrap();
        hps.perform_command(Command::EraseStage1).unwrap();
        hps.wait_ready().unwrap();
        hps.perform_command(Command::Launch1).unwrap();
        hps.wait_ready().unwrap();
        assert_eq!(hps.error().unwrap(), Error::Stage1NotFound);
    }

    fn test_load_dev_signed_stage1(&mut self) {
        let start = Instant::now();
        self.i2c
            .write_stage1(crate::firmware::ONE_TIME_INIT_LOADER_BYTES)
            .unwrap();
        info!("Stage1 write time {}ms", start.elapsed().as_millis());
        let mut hps = self.reset_hps().unwrap();
        let start = Instant::now();
        hps.perform_command(Command::Launch1).unwrap();
        // Immediately after receiving the Launch1 command, the HPS should
        // disable I2C, preventing us from sending any further requests while
        // it's performing the signature check.
        assert!(hps.status().is_err());
        hps.wait_ready().unwrap();
        assert_eq!(hps.error().unwrap(), Error::Stage1InvalidSignature);
        info!("Signature check time {}ms", start.elapsed().as_millis());
    }

    /// Resets the HPS then waits for stage0 to start and returns an interface
    /// to it.
    fn reset_hps(&mut self) -> Result<HpsAdapter> {
        self.i2c
            .open_hps()
            .unwrap()
            .perform_command(Command::Reset)
            .unwrap();
        self.i2c.open_hps()
    }
}
