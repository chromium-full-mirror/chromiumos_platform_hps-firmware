// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::time::Duration;
use std::time::Instant;

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::APPLICATION_BYTES;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use fpga_app::Features;
use hps_interface::Hps;
use mcu_common::commands::Command;
use mcu_common::registers;
use mcu_common::Status;

pub(crate) struct PostInstallTester<'a, I> {
    i2c: HpsI2c<'a, I>,
    config: crate::PostInstallTestConfig,
}

impl<'a, I, E> PostInstallTester<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn run(
        i2c: &mut I,
        test_config: &crate::PostInstallTestConfig,
        global_config: &Config,
    ) -> Result<()> {
        let i2c = HpsI2c::new(i2c, global_config.clone());
        let mut tester = PostInstallTester {
            i2c,
            config: test_config.clone(),
        };
        tester.write_firmware()?;
        tester.test_fpga_spi_flash_reads()?;
        Ok(())
    }

    fn write_firmware(&mut self) -> Result<()> {
        if !self.config.skip_firmware_writes {
            self.i2c.write_stage1(APPLICATION_BYTES)?;
            self.i2c.execute_stage1().context("Execute stage1")?;
            if !self.config.skip_bitstream_write {
                self.i2c.write_gateware().context("Write gateware")?;
            }
            self.i2c.write_soc_rom().context("Write FPGA ROM")?;
        }
        Ok(())
    }

    fn test_fpga_spi_flash_reads(&mut self) -> Result<()> {
        println!("Writing/verifying test data");
        self.i2c.open_hps()?.perform_command(Command::Reset)?;
        self.i2c.execute_stage1().context("Execute stage1")?;
        let mut hps = self.i2c.open_hps()?;
        hps.perform_command(Command::WriteSpiFlashTestData)?;
        while hps.status()?.contains(Status::COMMAND_IN_PROGRESS) {}
        hps.check_errors()?;
        println!("Test data written/verified");

        self.i2c
            .execute_application()
            .context("Execute application")?;
        println!("HPS application started");
        let mut hps = self.i2c.open_hps()?;
        hps.write_register(
            registers::ENABLED_FEATURES,
            Features::SPI_FLASH_READ_TEST.bits(),
        )?;
        let mut iteration_base = 0;
        let mut last_loop_count = 0;
        let mut last_report = Instant::now();
        let start = Instant::now();
        loop {
            let loop_count = hps.read_register(registers::FPGA_LOOP_COUNT)?;
            let bad_byte_count = hps.read_register(registers::USER_PRESENT_STATUS)?;
            if loop_count != last_loop_count {
                last_report = Instant::now();
                if loop_count < last_loop_count {
                    iteration_base += 1 << 16;
                }
                last_loop_count = loop_count;
                let iterations = iteration_base + (loop_count as u32);
                println!(
                    "{} iterations with {} errors so far. {:0.1}MB/s",
                    iterations,
                    bad_byte_count,
                    iterations as f64 / start.elapsed().as_secs_f64()
                );
            } else if last_report.elapsed() > Duration::from_secs(60) {
                if last_loop_count == 0 && iteration_base == 0 {
                    bail!("FPGA program failed to start");
                }
                bail!("FPGA program appears to have crashed");
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
