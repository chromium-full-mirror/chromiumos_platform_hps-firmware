// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::convert::TryFrom;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use factory_tester_common::EXPECTED_CHIP_ID;
use factory_tester_common::FACTORY_BOOTLOADER_I2C_ADDRESS;
use factory_tester_common::MCU_LOAD_ADDRESS;
use factory_tester_common::MCU_TESTER_I2C_ADDRESS;
use log::info;
use mcu_common::HPS_ADDRESS;
use stm32_bootloader_client::Stm32;

const MCU_CODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu.bin"));

/// How long we'll wait for the MCU to finish testing. Worst-cast scenario is
/// when there's already something on the SPI flash, but it's not what we want
/// to be there, so we've got to erase it.
const TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn run<E, I>(i2c: &mut I) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: core::fmt::Debug,
{
    let mut delay = linux_embedded_hal::Delay;
    // Check to see if we're connected to a HPS that already has firmware
    // loaded. For now, we don't support testing devices that already have
    // firmware loaded.
    // TODO: Support testing such devices that already have stage0 loaded.
    info!("Checking for existing HPS firmware");
    let mut response = [0u8; 2];
    if i2c.write_read(HPS_ADDRESS, &[0x80], &mut response).is_ok()
        && u16::from_be_bytes(response) == mcu_common::hps_magic_code()
    {
        bail!("Detected HPS that already has firmware loaded.");
    }

    let stm32_config = stm32_bootloader_client::Config::i2c_address(FACTORY_BOOTLOADER_I2C_ADDRESS);
    let mut stm32 = Stm32::new(i2c, &mut delay, stm32_config);

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
    stm32
        .write_bulk(MCU_LOAD_ADDRESS, MCU_CODE, |_| {})
        .map_err(|error| anyhow!("Failed to write MCU memory: {}", &error))?;

    // Start execution of MCU test program.
    info!("Starting MCU test program");
    stm32
        .go(MCU_LOAD_ADDRESS)
        .map_err(|error| anyhow!("Failed to start MCU program: {}", &error))?;

    // Without this sleep, we sometimes get stuck in the i2c.read call below.
    // It's not clear why.
    std::thread::sleep(Duration::from_millis(2000));

    // Read the result from the MCU test program (with retry until timeout).
    info!("Waiting for test result");
    let start = std::time::Instant::now();
    let mut result = [0u8; 1];
    loop {
        match i2c.read(MCU_TESTER_I2C_ADDRESS, &mut result) {
            Ok(_) => break,
            Err(error) => {
                if start.elapsed() > TIMEOUT {
                    anyhow!("Failed to read test result from MCU: {:?}", error);
                }
            }
        }
    }

    let test_result = result[0];
    if test_result != factory_tester_common::TEST_STATUS_OK {
        match factory_tester_common::Error::try_from(test_result) {
            Ok(error) => bail!("MCU reported error: {:?}", error),
            Err(_) => bail!("MCU reported unknown status: {}", test_result),
        }
    }
    println!("PASS");
    Ok(())
}
