// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::firmware::PRE_INSTALL_TESTER_MCU_CODE;
use crate::firmware::PRE_INSTALL_TESTER_MCU_CODE_STAGE1;
use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::TestConfig;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use factory_tester_common::TestStatus;
use factory_tester_common::ALLOWED_SPI_FLASH_IDS;
use factory_tester_common::EXPECTED_CHIP_ID;
use factory_tester_common::MCU_LOAD_ADDRESS;
use factory_tester_common::MCU_TESTER_I2C_ADDRESS;
use factory_tester_common::TEST_STATUS_SIZE;
use hps_interface::Hps;
use log::info;
use mcu_common::commands::Command;
use mcu_common::registers;
use mcu_common::Status;
use std::convert::TryFrom;
use std::time::Duration;

/// How long we'll wait for the MCU to finish testing. Worst-cast scenario is
/// when there's already something on the SPI flash, but it's not what we want
/// to be there, so we've got to erase it.
const TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn run<E, I>(raw_i2c: &mut I, config: &TestConfig, global_config: &Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(raw_i2c, global_config.clone());
    // If the HPS is running HPS firmware, start by resetting in order to make
    // sure we're talking to a bootloader. That bootloader might be stage0 or
    // the system bootloader.
    if let Ok(mut hps) = i2c.open_hps() {
        hps.perform_command(Command::Reset)?;
    }
    // Check to see if we're connected to a HPS that already has firmware
    // loaded.
    info!("Checking for existing HPS firmware");
    let mut stage0_present = false;
    if let Ok(mut hps) = i2c.open_hps() {
        stage0_present = true;
        if hps.read_register(registers::HARDWARE_VERSION)? == 0 {
            bail!("HPS is running firmware, but it's not stage0 - cannot perform test");
        }
        if hps.status()?.contains(Status::WPON) {
            bail!("Firmware write protect is asserted - Cannot perform test");
        }
        info!("Writing test program using stage0");
        hps.write_firmware(PRE_INSTALL_TESTER_MCU_CODE_STAGE1)
            .context("Failed to write test program via stage0")?;
        hps.check_errors()
            .context("Failed to read status after writing test program")?;
        if hps.stage0_version()? < 4 {
            info!("Write complete, resetting HPS");
            i2c.reset_to_bootloader()
                .context("Failed to reset to stage0")?;
            hps = i2c.open_hps()?;
            info!("Launching test program");
        } else {
            info!("Write complete, launching test program");
        }
        hps.perform_command(Command::Launch1)?;
    } else {
        let mut stm32 = i2c.open_bootloader()?;

        info!("Checking STM32 chip ID");
        match stm32.get_chip_id() {
            Ok(EXPECTED_CHIP_ID) => {
                // Nothing to do.
            }
            Ok(id) => {
                bail!("Unexpected chip ID: {}", id);
            }
            Err(error) => {
                bail!("Failed to read chip ID: {:?}", &error);
            }
        }

        // Flash the MCU test program.
        info!("Sending MCU test program");
        i2c.write(
            "test firmware",
            MCU_LOAD_ADDRESS,
            PRE_INSTALL_TESTER_MCU_CODE,
        )
        .context("Failed to write test firmware")?;

        let mut stm32 = i2c.open_bootloader()?;

        // Start execution of MCU test program.
        info!("Starting MCU test program");
        stm32
            .go(MCU_LOAD_ADDRESS)
            .map_err(|error| anyhow!("Failed to start MCU program: {}", &error))?;
    }

    // Without this sleep, we sometimes get stuck in the i2c.read call below.
    // It's not clear why.
    std::thread::sleep(Duration::from_millis(2000));

    // Read the result from the MCU test program (with retry until timeout).
    info!("Waiting for test result");
    let start = std::time::Instant::now();
    let mut buffer = [0u8; TEST_STATUS_SIZE];
    loop {
        match i2c.i2c.read(MCU_TESTER_I2C_ADDRESS, &mut buffer) {
            Ok(_) => break,
            Err(error) => {
                if start.elapsed() > TIMEOUT {
                    bail!("Failed to read test result from MCU: {:?}", error);
                }
            }
        }
    }

    let status = TestStatus::from_bytes(&buffer);
    if status.error_code != factory_tester_common::TEST_STATUS_OK {
        match factory_tester_common::Error::try_from(status.error_code) {
            Ok(error) => bail!("MCU reported error: {:?}", error),
            Err(_) => bail!("MCU reported unknown status: {}", status.error_code),
        }
    }

    if !config.skip_spi_flash_id_check
        && !ALLOWED_SPI_FLASH_IDS.contains(&status.outputs.spi_flash_id)
    {
        bail!(
            "Unsupported SPI flash ID 0x{:x}, [0x{:x}, 0x{:x}]",
            status.outputs.spi_flash_id.manufacturer_id,
            status.outputs.spi_flash_id.device_id[0],
            status.outputs.spi_flash_id.device_id[1]
        );
    }

    if stage0_present {
        info!("Erasing test program");
        // Erase our test program. If we don't do this, then it'll mess with
        // anything that performs Launch1 in order to check the firmware
        // version, since our test program uses a very basic I2C protocol.
        let mut hps = i2c.open_hps()?;
        hps.perform_command(Command::EraseStage1)
            .context("Failed to erase test program")?;
    }

    info!("Pre-install test passed");
    Ok(())
}
