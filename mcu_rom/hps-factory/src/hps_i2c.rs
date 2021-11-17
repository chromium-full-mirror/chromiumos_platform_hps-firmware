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
use linux_embedded_hal::Delay;
use mcu_common::commands::Command;
use mcu_common::memory_banks;
use mcu_common::Status;
use mcu_common::APPLICATION_START_ADDRESS;
use mcu_common::APPLICATION_VECTOR_TABLE_ADDRESS;
use mcu_common::HPS_ADDRESS;
use std::time::Duration;
use std::time::Instant;
use stm32_bootloader_client::Stm32;

#[cfg(feature = "image-transfer")]
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
        Self { i2c, config }
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
        let stage = self.open_hps()?.stage()?;
        if stage != Stage::Stage0 {
            bail!(
                "Need either stage0 or the system bootloader, but got: {:?}",
                stage
            );
        }
        self.write_memory_bank("stage1", memory_banks::MCU_RW, 0, program_bytes)
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
            show_progress(
                &format!("Writing {} ({}KiB)", name, program_bytes.len() / 1024),
                |bar| {
                    Ok(stm32.write_bulk(address, program_bytes, |progress| {
                        bar.reach_percent(progress.percent_complete as i32);
                    })?)
                },
            )?;

            if verify_writes {
                Self::verify(&mut stm32, name, address, program_bytes)?;
            }
        }

        Ok(())
    }

    pub(crate) fn write_memory_bank(
        &mut self,
        section_name: &str,
        bank: u8,
        start_address: u32,
        bytes: &[u8],
    ) -> Result<()> {
        let mut hps = self.open_hps()?;
        hps.check_errors()?;
        show_progress(
            &format!("Writing {} ({}KiB)", section_name, bytes.len() / 1024),
            |bar| {
                let mut address = start_address;
                for chunk in bytes.chunks(256) {
                    hps.wait_memory_bank_available(bank)?;
                    hps.write_memory(bank, address, chunk)?;
                    address += chunk.len() as u32;
                    let percent = (address - start_address) as f64 * 100f64 / (bytes.len() as f64);
                    bar.reach_percent(percent as i32);
                }
                Ok(())
            },
        )?;
        hps.check_errors()?;
        Ok(())
    }

    #[cfg(feature = "image-transfer")]
    pub(crate) fn write_gateware(&mut self) -> Result<()> {
        self.write_memory_bank(
            "gateware bitstream",
            memory_banks::SPI_FLASH,
            0,
            fpga_firmware::GATEWARE_BITSTREAM,
        )
    }

    #[cfg(feature = "image-transfer")]
    pub(crate) fn write_soc_rom(&mut self) -> Result<()> {
        self.write_memory_bank(
            "SOC ROM",
            memory_banks::SPI_FLASH,
            fpga_firmware::SOC_ROM_START_ADDRESS,
            fpga_firmware::SOC_ROM_BYTES,
        )
    }

    /// Executes stage1, either using the STM32 bootloader if active, or stage0
    /// if that's active.
    pub(crate) fn execute_stage1(&mut self) -> Result<()> {
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
        let mut hps = self.open_hps()?;
        if !hps.status()?.contains(Status::STAGE1) {
            bail!("Stage1 failed to start");
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
        show_progress(
            &format!("Verifying {} ({}KiB)", name, program_bytes.len() / 1024),
            |bar| {
                Ok(stm32.verify(address, program_bytes, |progress| {
                    bar.reach_percent(progress.percent_complete as i32);
                })?)
            },
        )
    }
}

fn show_progress(
    title: &str,
    mut callback: impl FnMut(&mut progress::Bar) -> Result<()>,
) -> Result<()> {
    let mut bar = progress::Bar::new();
    bar.set_job_title(title);
    let result = callback(&mut bar);
    if result.is_ok() {
        bar.reach_percent(100);
    }
    println!();
    result
}
