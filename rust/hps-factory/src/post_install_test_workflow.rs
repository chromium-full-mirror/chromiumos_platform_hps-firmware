// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::time::Duration;
use std::time::Instant;

use crate::hps_i2c::HpsI2c;
use crate::Config;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use fpga_app::Features;
use hps_interface::Hps;
use indicatif::ProgressBar;
use log::info;
use mcu_common::commands::Command;
use mcu_common::registers::Register;
use mcu_common::Status;

pub(crate) struct PostInstallTester<'a, I: Send> {
    i2c: HpsI2c<'a, I>,
    config: crate::PostInstallTestConfig,
}

impl<'a, I, E> PostInstallTester<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn run(
        i2c: &mut I,
        test_config: &crate::PostInstallTestConfig,
        global_config: &Config,
    ) -> Result<()> {
        let mut i2c = HpsI2c::new(i2c, global_config.clone());
        i2c.write_firmware()?;
        let mut tester = PostInstallTester {
            i2c,
            config: test_config.clone(),
        };
        tester.test_i2c_integrity()?;
        tester.test_fpga_spi_flash_reads()?;
        tester.test_fpga_mcu_comms()?;
        tester.test_mcu_to_camera_i2c()?;
        tester.test_camera_data_bus()?;
        info!("Post-install test passed");
        Ok(())
    }

    fn test_i2c_integrity(&mut self) -> Result<()> {
        if self.config.host_i2c_iterations == 0 {
            return Ok(());
        }
        info!("Checking host I2C bus");
        self.i2c.reset_to_bootloader()?;
        let mut hps = self.i2c.open_hps()?;
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
        info!("Checking FPGA<->MCU comms");
        self.i2c.reset_to_bootloader()?;
        self.i2c
            .execute_application()
            .context("Failed to execute application")?;
        let mut hps = self.i2c.open_hps()?;
        let bar = ProgressBar::new(self.config.fpga_mcu_iterations.into());
        hps.write_register(
            Register::EnabledFeatures,
            Features::MCU_FPGA_COMM_TEST.bits(),
        )?;
        // Make sure the fpga_app has finished initializing and has started
        // executing the test loop, otherwise our timing will be out.
        while hps.read_register(Register::FpgaLoopCount)? < 5 {
            std::thread::sleep(Duration::from_millis(100));
        }
        let start = Instant::now();
        let start_loop_count = hps.read_register(Register::FpgaLoopCount)?;
        loop {
            let loop_count = hps.read_register(Register::FpgaLoopCount)?;
            let error_count = hps.read_register(Register::UserPresentStatus)?;
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
        let final_loop_count = hps.read_register(Register::FpgaLoopCount)?;
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
        info!(
            "Average transfer speed: {} bytes/sec",
            (bytes_transferred as f64 / start.elapsed().as_secs_f64()) as u32
        );
        Ok(())
    }

    fn test_fpga_spi_flash_reads(&mut self) -> Result<()> {
        if self.config.fpga_spi_read_iterations == 0 {
            return Ok(());
        }
        info!("Writing/verifying test data");
        self.i2c.reset_to_bootloader()?;
        self.i2c
            .execute_stage1()
            .context("Failed to execute stage1")?;
        let mut hps = self.i2c.open_hps()?;
        hps.perform_command(Command::WriteSpiFlashTestData)?;
        while hps.status()?.contains(Status::COMMAND_IN_PROGRESS) {}
        hps.check_errors()?;
        info!("Test data written/verified");
        drop(hps);

        self.i2c
            .execute_application()
            .context("Failed to execute application")?;
        info!("Testing SPI flash reads from FPGA");
        let mut hps = self.i2c.open_hps()?;
        hps.write_register(
            Register::EnabledFeatures,
            Features::SPI_FLASH_READ_TEST.bits(),
        )?;
        let bar = ProgressBar::new(self.config.fpga_spi_read_iterations.into());
        let mut last_report = Instant::now();
        let mut last_loop_count = 0;
        loop {
            let loop_count = hps.read_register(Register::FpgaLoopCount)?;
            let bad_byte_count = hps.read_register(Register::UserPresentStatus)?;
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

    fn test_mcu_to_camera_i2c(&mut self) -> Result<()> {
        if self.config.camera_i2c_iterations == 0 {
            return Ok(());
        }
        info!("Testing MCU<->camera I2C communication");
        self.i2c.reset_to_bootloader()?;
        self.i2c
            .execute_application()
            .context("Failed to execute application")?;
        let mut hps = self.i2c.open_hps()?;
        hps.write_register(
            Register::CameraTestIterations,
            self.config.camera_i2c_iterations,
        )?;
        let bar = ProgressBar::new(self.config.camera_i2c_iterations as u64);
        loop {
            let remaining = hps.read_register(Register::CameraTestIterations)?;
            hps.check_errors()?;
            bar.set_position((self.config.camera_i2c_iterations - remaining) as u64);
            if remaining == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bar.finish();
        Ok(())
    }

    fn test_camera_data_bus(&mut self) -> Result<()> {
        if self.config.camera_data_bus_iterations == 0 {
            return Ok(());
        }
        info!("Checking camera<->FPGA data bus");
        self.i2c.reset_to_bootloader()?;
        self.i2c
            .execute_application()
            .context("Failed to execute application")?;
        let mut hps = self.i2c.open_hps()?;
        let bar = ProgressBar::new(self.config.camera_data_bus_iterations.into());
        hps.write_register(Register::EnabledFeatures, Features::CAMERA_DATA_TEST.bits())?;
        loop {
            let loop_count = hps.read_register(Register::FpgaLoopCount)?;
            let error_count = hps.read_register(Register::UserPresentStatus)?;
            if error_count > 0 {
                bail!(
                    "Got {} errors after {} iterations of camera data test",
                    error_count,
                    loop_count
                );
            }
            bar.set_position(loop_count.into());
            if loop_count >= self.config.camera_data_bus_iterations {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bar.finish();
        Ok(())
    }
}
