// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Config;
use anyhow::bail;
use anyhow::Context;
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
use log::info;
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

const FPGA_ROM_START_ADDRESS: u32 = 2 * 1024 * 1024;

/// How long we'll wait for the system bootloader or HPS stage0/stage1 to become
/// responsive to I2C requests.
const STARTUP_TIMOUT: Duration = Duration::from_millis(1000);

/// How long we'll wait for the soft CPU to start.
const FPGA_START_TIMEOUT: Duration = Duration::from_secs(5);

/// Path to the sysfs node of the HPS kernel driver.
const HPS_I2C_SYSFS_PATH: &str = "/sys/bus/i2c/drivers/cros-hps";

// The i2c device id for HPS.
const HPS_I2C_DEVICE_ID: &str = "i2c-GOOG0020:00";

/// An I2C interface to the HPS. Allows access to both the system bootloader and
/// HPS functionality.
pub(crate) struct HpsI2c<'a, I> {
    pub(crate) i2c: &'a mut I,
    config: Config,
    pub(crate) erase_stage1_allowed: bool,
}

impl<'a, I, E> HpsI2c<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + 'a,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn new(i2c: &'a mut I, config: Config) -> Self {
        let hps_i2c = Self {
            i2c,
            config,
            erase_stage1_allowed: true,
        };
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
        let erase_stage1_allowed = self.erase_stage1_allowed;
        // We need to be running some kind of bootloader in order to write stage1.
        self.reset_to_bootloader()?;
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
        // Old versions of stage0 don't support the EraseStage1 command. In that
        // case, we don't need to use the command, since the MCU flash will get
        // automatically erased as we write.
        if erase_stage1_allowed {
            info!("Erasing RW region");
            hps.perform_command(Command::EraseStage1)?;
        }
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
                info!("Something already written to flash, erasing...");
                stm32.erase_flash()?;
            }

            // Write flash
            info!("Writing {} ({}KiB)", name, program_bytes.len() / 1024);
            let bar = ProgressBar::new(program_bytes.len() as u64);
            stm32.write_bulk(address, program_bytes, |progress| {
                bar.set_position(progress.bytes_complete as u64);
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
        info!("Writing {} ({}KiB)", section_name, bytes.len() / 1024);
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
        let fpga_bitstream = self.config.fpga_bitstream()?;
        let mut hps = self.open_hps()?;
        info!("Erasing SPI flash");
        hps.perform_command(Command::EraseSpiFlash)?;
        hps.wait_memory_bank_available(memory_banks::SPI_FLASH)?;
        Self::write_memory_bank(
            &mut hps,
            "gateware bitstream",
            memory_banks::SPI_FLASH,
            0,
            &fpga_bitstream,
        )
    }

    pub(crate) fn write_soc_rom(&mut self) -> Result<()> {
        let fpga_rom = self.config.fpga_rom()?;
        let mut hps = self.open_hps()?;
        // TODO(dcallagh): this only works without erasing SPI flash
        // because we are still also erasing each individual block on write.
        Self::write_memory_bank(
            &mut hps,
            "SOC ROM",
            memory_banks::SPI_FLASH,
            FPGA_ROM_START_ADDRESS,
            &fpga_rom,
        )
    }

    /// Executes a stage1, but doesn't attempt to check that it's running. Can
    /// be used for stage1 binaries that don't support I2C.
    pub(crate) fn execute_non_i2c_stage1(&mut self) -> Result<()> {
        let mut hps = self.open_hps()?;
        if hps.status()?.contains(Status::ANONVER) || hps.stage()? != Stage::Stage0 {
            self.reset_to_bootloader()?;
        }
        if let Ok(mut stm32) = self.open_bootloader() {
            stm32.go(APPLICATION_VECTOR_TABLE_ADDRESS)?;
            return Ok(());
        }
        let mut hps = self.open_hps()?;
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
        hps.check_errors().context("Errors reported by stage1")?;
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
        info!("Verifying {} ({}KiB)", name, program_bytes.len() / 1024);
        let bar = ProgressBar::new(program_bytes.len() as u64);
        stm32.verify(address, program_bytes, |progress| {
            bar.set_position(progress.bytes_complete as u64);
        })?;
        bar.finish();
        Ok(())
    }

    /// Resets to the bootloader - either the sytem bootloader or stage0,
    /// whichever is active.
    pub(crate) fn reset_to_bootloader(&mut self) -> Result<()> {
        // Check if the system bootloader is already running.
        if self
            .i2c
            .read(FACTORY_BOOTLOADER_I2C_ADDRESS, &mut [0])
            .is_ok()
        {
            return Ok(());
        }
        self.open_hps()?.perform_command(Command::Reset)?;

        // Wait until one or other bootloader responds on I2C.
        let mut start = Instant::now();
        let mut has_tried_power_cycle = false;
        loop {
            std::thread::sleep(Duration::from_millis(20));
            if self
                .i2c
                .read(FACTORY_BOOTLOADER_I2C_ADDRESS, &mut [0])
                .is_ok()
                || self.i2c.read(HPS_ADDRESS, &mut [0]).is_ok()
            {
                return Ok(());
            }
            if start.elapsed() > STARTUP_TIMOUT {
                // Attempt to power-cycle the HPS. This will only work if running as root.
                if !has_tried_power_cycle && self.power_cycle_hps().is_ok() {
                    info!("HPS successfully power-cycled");
                    has_tried_power_cycle = true;
                    start = Instant::now();
                } else {
                    bail!("Neither the system bootloader nor HPS stage0 returned after reset");
                }
            }
        }
    }

    fn power_cycle_hps(&self) -> Result<()> {
        let gpio_offset = &self.config.power_gpio_offset;
        if gpio_offset.is_empty() {
            bail!("Power control is disabled");
        }
        // Note, we don't actually provide any output from gpioset to the user here.
        // Running gpioset to power-cycle the HPS is a last-resort. In many cases
        // gpioset might not be available or might not run (non-root).
        if !std::process::Command::new("gpioset")
            .arg("0")
            .arg(format!("{}=0", gpio_offset))
            .output()?
            .status
            .success()
        {
            bail!("gpioset returned non-zero value");
        }
        std::thread::sleep(Duration::from_millis(100));
        if !std::process::Command::new("gpioset")
            .arg("0")
            .arg(format!("{}=1", gpio_offset))
            .output()?
            .status
            .success()
        {
            bail!("gpioset returned non-zero value");
        }
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
