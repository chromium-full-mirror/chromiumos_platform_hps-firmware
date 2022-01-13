// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Config;
use anyhow::bail;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use factory_tester_common::EXPECTED_CHIP_ID;
use factory_tester_common::FACTORY_BOOTLOADER_I2C_ADDRESS;
use hps_interface::BorrowedI2c;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use hps_interface::Stage;
use indicatif::ProgressBar;
use linux_embedded_hal::Delay;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::registers;
use mcu_common::Status;
use mcu_common::APPLICATION_START_ADDRESS;
use mcu_common::APPLICATION_VECTOR_TABLE_ADDRESS;
use mcu_common::HPS_ADDRESS;
use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;
use stm32_bootloader_client::Stm32;

mod fpga_firmware {
    pub(super) const GATEWARE_BITSTREAM: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/hps_platform.bit"));
    pub(super) const SOC_ROM_BYTES: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/soc_rom.bin"));
    pub(super) const SOC_ROM_START_ADDRESS: u32 = 2 * 1024 * 1024;
}

/// How long we'll wait for the system bootloader or HPS stage0/stage1 to become
/// responsive to I2C requests.
const STARTUP_TIMOUT: Duration = Duration::from_millis(1000);

/// How long we'll wait for the soft CPU to start.
const FPGA_START_TIMEOUT: Duration = Duration::from_secs(5);

/// Path to the sysfs node of the HPS kernel driver.
const HPS_I2C_SYSFS_PATH: &str = "/sys/bus/i2c/drivers/hps";

// The i2c device id for HPS.
const HPS_I2C_DEVICE_ID: &str = "i2c-GOOG0020:00";

/// An I2C interface to the HPS. Allows access to both the system bootloader and
/// HPS functionality.
pub(crate) struct HpsI2c<'a, I> {
    i2c: &'a mut I,
    config: Config,
}

impl<'a, I, E> HpsI2c<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + 'a,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn new(i2c: &'a mut I, config: Config) -> Self {
        let hps_i2c = Self { i2c, config };
        // Since the HPS kernel driver takes exclusive ownership of the HPS I2C
        // address, we must unbind the driver while this tool is running. The
        // driver is bound again after the interface is closed -- see `drop`
        // below.
        hps_i2c.bind_kernel_driver(false).unwrap();
        hps_i2c
    }

    /// Writes `program_bytes` into the stage1 slot using either the STM32
    /// bootloader if active, or stage0 if that's active.
    pub(crate) fn write_stage1(&mut self, program_bytes: &[u8]) -> Result<()> {
        // If we're running HPS firmware, but it's not stage0, then we need to
        // reset in order to get to either the system bootloader or stage0.
        if let Ok(mut hps) = self.open_hps() {
            if hps.stage()? != Stage::Stage0 {
                hps.perform_command(Command::Reset)?;
            }
        }
        if self.open_bootloader().is_ok() {
            return self.write("stage1", APPLICATION_START_ADDRESS, program_bytes);
        }
        let mut hps = self.open_hps()?;
        let stage = hps.stage()?;
        if stage != Stage::Stage0 {
            bail!(
                "Need either stage0 or the system bootloader, but got: {:?}",
                stage
            );
        }
        println!("Erasing RW region");
        hps.perform_command(Command::EraseStage1)?;
        hps.wait_memory_bank_available(memory_banks::MCU_RW)?;
        Self::write_memory_bank(&mut hps, "stage1", memory_banks::MCU_RW, 0, program_bytes)
    }

    /// Writes a `program_bytes` to flash at `address` using the STM32
    /// bootloader.
    pub(crate) fn write(&mut self, name: &str, address: u32, program_bytes: &[u8]) -> Result<()> {
        assert!(!program_bytes.is_empty());
        let verify_writes = self.config.verify_writes;
        let mut stm32 = self.open_bootloader()?;

        if Self::verify(&mut stm32, name, address, program_bytes).is_err() {
            let mut current_data = [0u8; 128];
            stm32.read_memory(address, &mut current_data)?;
            if current_data != [0xffu8; 128] {
                println!("Something already written to flash, erasing...");
                stm32.erase_flash()?;
            }

            // Write flash
            println!("Writing {} ({}KiB)", name, program_bytes.len() / 1024);
            let bar = ProgressBar::new(100);
            stm32.write_bulk(address, program_bytes, |progress| {
                bar.set_position(progress.percent_complete as u64);
            })?;
            bar.finish();

            if verify_writes {
                Self::verify(&mut stm32, name, address, program_bytes)?;
            }
        }

        Ok(())
    }

    pub(crate) fn write_memory_bank(
        hps: &mut HpsAdapter<BorrowedI2c<I, E>, E>,
        section_name: &str,
        bank: u8,
        start_address: u32,
        bytes: &[u8],
    ) -> Result<()> {
        hps.check_errors()?;
        if !hps.is_memory_bank_available(bank) {
            bail!("Cannot write to memory bank {}", bank);
        }
        println!("Writing {} ({}KiB)", section_name, bytes.len() / 1024);
        let bar = ProgressBar::new(bytes.len() as u64);
        let mut address = start_address;
        for chunk in bytes.chunks(256) {
            hps.write_memory(bank, address, chunk)?;
            hps.wait_memory_bank_available(bank)?;
            address += chunk.len() as u32;
            bar.inc(chunk.len() as u64);
        }
        bar.finish();
        hps.check_errors()?;
        Ok(())
    }

    pub(crate) fn write_gateware(&mut self) -> Result<()> {
        let mut hps = self.open_hps()?;
        println!("Erasing SPI flash");
        hps.perform_command(Command::EraseSpiFlash)?;
        hps.wait_memory_bank_available(memory_banks::SPI_FLASH)?;
        Self::write_memory_bank(
            &mut hps,
            "gateware bitstream",
            memory_banks::SPI_FLASH,
            0,
            fpga_firmware::GATEWARE_BITSTREAM,
        )
    }

    pub(crate) fn write_soc_rom(&mut self) -> Result<()> {
        let mut hps = self.open_hps()?;
        // TODO(dcallagh): this only works without erasing SPI flash
        // because we are still also erasing each individual block on write.
        Self::write_memory_bank(
            &mut hps,
            "SOC ROM",
            memory_banks::SPI_FLASH,
            fpga_firmware::SOC_ROM_START_ADDRESS,
            fpga_firmware::SOC_ROM_BYTES,
        )
    }

    /// Executes a stage1, but doesn't attempt to check that it's running. Can
    /// be used for stage1 binaries that don't support I2C.
    pub(crate) fn execute_non_i2c_stage1(&mut self) -> Result<()> {
        if let Ok(mut stm32) = self.open_bootloader() {
            stm32.go(APPLICATION_VECTOR_TABLE_ADDRESS)?;
            return Ok(());
        }
        let mut hps = self.open_hps()?;
        let status = hps.status()?;
        if status.contains(Status::STAGE1) {
            // Stage1 is already running.
            return Ok(());
        }
        // If stage0 is running, but stage1 hasn't been verified, or if the
        // application is running, then reset to get back to stage0.
        if status.contains(Status::ANONVER) || status.contains(Status::APPLREADY) {
            hps.perform_command(Command::Reset)?;
            // It takes about 690ms to signature-check stage1.
            std::thread::sleep(Duration::from_millis(700));
            hps = self.open_hps()?;
        }
        let status = hps.status()?;
        if status.contains(Status::ANONVER) {
            if status.contains(Status::WPON) {
                bail!("Stage1 is not signed and firmware write-protect is asserted");
            } else {
                bail!("Stage1 is invalid");
            }
        }
        hps.perform_command(Command::Launch1)?;
        Ok(())
    }

    /// Executes stage1, either using the STM32 bootloader if active, or stage0
    /// if that's active.
    pub(crate) fn execute_stage1(&mut self) -> Result<()> {
        self.execute_non_i2c_stage1()?;

        // Check that it started.
        let mut hps = self.open_hps()?;
        if !hps.status()?.contains(Status::STAGE1) {
            bail!("Stage1 failed to start");
        }
        Ok(())
    }

    /// Executes the MCU application if it's not already running.
    pub(crate) fn execute_application(&mut self) -> Result<()> {
        if let Ok(mut hps) = self.open_hps() {
            if hps.stage()? == Stage::Application {
                return Ok(());
            }
        }
        self.execute_stage1()?;
        let mut hps = self.open_hps()?;
        hps.perform_command(Command::LaunchApp)?;
        while hps.stage()? != Stage::Application {
            hps.check_errors()?;
        }
        let start = Instant::now();
        loop {
            if hps.read_register(registers::FPGA_BOOT_COUNT)? > 0 {
                break;
            }
            if start.elapsed() > FPGA_START_TIMEOUT {
                bail!(
                    "FPGA soft CPU failed to start within {}s",
                    FPGA_START_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    pub(crate) fn open_bootloader(&mut self) -> Result<Stm32<I, Delay>> {
        self.wait_i2c_ready(FACTORY_BOOTLOADER_I2C_ADDRESS, "STM32 system bootloader")?;
        let mut stm32_config =
            stm32_bootloader_client::Config::i2c_address(FACTORY_BOOTLOADER_I2C_ADDRESS);
        // Datasheet for stm32g071 says 41.1ms.
        stm32_config.mass_erase_max_ms = 42;
        // Leaking a zero-sized type is free and saves us passing in a delay.
        assert!(std::mem::size_of::<Delay>() == 0);
        let delay = Box::leak(Box::new(Delay));
        let mut stm32 = Stm32::new(self.i2c, delay, stm32_config);

        // Double-check that we've got the device we expected.
        let chip_id = stm32.get_chip_id()?;
        if chip_id != EXPECTED_CHIP_ID {
            bail!("Found unsupported chip ID: 0x{:x}", chip_id);
        }

        Ok(stm32)
    }

    pub(crate) fn open_hps(&mut self) -> Result<HpsAdapter<BorrowedI2c<I, E>, E>> {
        self.wait_i2c_ready(HPS_ADDRESS, "HPS I2C")?;
        HpsAdapter::from_ref(self.i2c)
    }

    /// Waits until the specified I2C address starts responding or until the
    /// timeout is reached.
    fn wait_i2c_ready(&mut self, address: u8, name: &str) -> Result<()> {
        std::thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        loop {
            match self.i2c.read(address, &mut [0]) {
                Ok(_) => return Ok(()),
                Err(error) => {
                    if start.elapsed() > STARTUP_TIMOUT {
                        bail!("{} is not running: {}", name, error)
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }

    fn verify(
        stm32: &mut Stm32<I, Delay>,
        name: &str,
        address: u32,
        program_bytes: &[u8],
    ) -> Result<()> {
        println!("Verifying {} ({}KiB)", name, program_bytes.len() / 1024);
        let bar = ProgressBar::new(100);
        stm32.verify(address, program_bytes, |progress| {
            bar.set_position(progress.percent_complete as u64);
        })?;
        bar.finish();
        Ok(())
    }
}

impl<'a, I> HpsI2c<'a, I> {
    fn bind_kernel_driver(&self, bind: bool) -> Result<()> {
        #[cfg(feature = "mcp2221")]
        if self.config.mcp {
            return Ok(());
        }
        let driver_path = Path::new(HPS_I2C_SYSFS_PATH);
        if !driver_path.exists() {
            return Ok(());
        }
        let op = if bind { "bind" } else { "unbind" };
        fs::write(driver_path.join(op), HPS_I2C_DEVICE_ID)?;
        Ok(())
    }
}

impl<'a, I> Drop for HpsI2c<'a, I> {
    fn drop(&mut self) {
        // Let the kernel take ownership of HPS again.
        self.bind_kernel_driver(true).unwrap();
    }
}
