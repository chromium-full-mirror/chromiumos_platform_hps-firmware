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
use indicatif::ProgressBar;
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
        tester.test_i2c_integrity()?;
        tester.test_fpga_spi_flash_reads()?;
        tester.test_fpga_mcu_comms()?;
        println!("PASS");
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

    fn test_i2c_integrity(&mut self) -> Result<()> {
        if self.config.host_i2c_iterations == 0 {
            return Ok(());
        }
        let mut hps = self.i2c.open_hps()?;
        println!("Checking host I2C bus");
        let bar = ProgressBar::new(self.config.host_i2c_iterations);
        for i in 0..self.config.host_i2c_iterations {
            hps.check_magic()?;
            bar.set_position(i);
        }
        bar.finish();
        Ok(())
    }

    fn test_fpga_mcu_comms(&mut self) -> Result<()> {
        if self.config.fpga_mcu_iterations == 0 {
            return Ok(());
        }
        self.i2c.open_hps()?.perform_command(Command::Reset)?;
        self.i2c
            .execute_application()
            .context("Execute application")?;
        let mut hps = self.i2c.open_hps()?;
        println!("Checking FPGA<->MCU comms");
        let bar = ProgressBar::new(self.config.fpga_mcu_iterations.into());
        hps.write_register(
            registers::ENABLED_FEATURES,
            Features::MCU_FPGA_COMM_TEST.bits(),
        )?;
        // Make sure the fpga_app has finished initializing and has started
        // executing the test loop, otherwise our timing will be out.
        while hps.read_register(registers::FPGA_LOOP_COUNT)? < 5 {
            std::thread::sleep(Duration::from_millis(100));
        }
        let start = Instant::now();
        let start_loop_count = hps.read_register(registers::FPGA_LOOP_COUNT)?;
        loop {
            let loop_count = hps.read_register(registers::FPGA_LOOP_COUNT)?;
            let error_count = hps.read_register(registers::USER_PRESENT_STATUS)?;
            if error_count > 0 {
                bail!(
                    "Got {} errors after {} iterations of FPGA<->MCU comms test",
                    error_count,
                    loop_count
                );
            }
            bar.set_position(loop_count.into());
            if loop_count >= self.config.fpga_mcu_iterations {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let final_loop_count = hps.read_register(registers::FPGA_LOOP_COUNT)?;
        // Note, the * 2 is because each buffer gets transferred twice (from
        // FPGA->MCU then MCU->FPGA). There is some time spent by the FPGA
        // setting up the buffer then checking that the returned buffer is
        // correct. This time is included because excluding it would require
        // doing the timing on the soft CPU.
        let bytes_transferred = application::fpga::BUFFER_SIZE
            * fpga_app::MCU_COMMS_TESTS_PER_LOOP as usize
            * 2
            * (final_loop_count - start_loop_count) as usize;
        bar.finish();
        println!(
            "Average transfer speed: {} bytes/sec",
            (bytes_transferred as f64 / start.elapsed().as_secs_f64()) as u32
        );
        Ok(())
    }

    fn test_fpga_spi_flash_reads(&mut self) -> Result<()> {
        if self.config.fpga_spi_read_iterations == 0 {
            return Ok(());
        }
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
        println!("Testing SPI flash reads from FPGA");
        let mut hps = self.i2c.open_hps()?;
        hps.write_register(
            registers::ENABLED_FEATURES,
            Features::SPI_FLASH_READ_TEST.bits(),
        )?;
        let bar = ProgressBar::new(self.config.fpga_spi_read_iterations.into());
        let mut last_report = Instant::now();
        let mut last_loop_count = 0;
        loop {
            let loop_count = hps.read_register(registers::FPGA_LOOP_COUNT)?;
            let bad_byte_count = hps.read_register(registers::USER_PRESENT_STATUS)?;
            if bad_byte_count > 0 {
                bail!(
                    "FPGA got {} bad reads of SPI flash after {} iterations",
                    bad_byte_count,
                    loop_count
                );
            }
            bar.set_position(loop_count.into());
            if loop_count >= self.config.fpga_spi_read_iterations {
                break;
            }
            if loop_count != last_loop_count {
                last_loop_count = loop_count;
                last_report = Instant::now();
            } else if last_report.elapsed() > Duration::from_secs(60) {
                if last_loop_count == 0 {
                    bail!("FPGA program failed to start");
                }
                bail!("FPGA program appears to have crashed");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bar.finish();
        Ok(())
    }
}
