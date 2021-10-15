// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use embedded_hal::blocking::i2c::Read;
use mcp2221::GpioConfig;
use mcp2221::I2c;
use mcp2221::I2cConfig;
use mcp2221::Result;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        println!("Error: {}", error);
    }
}

fn run() -> Result<()> {
    let mut config = I2cConfig::default();
    config.speed_hz = 400_000;
    config.timeout = Duration::from_millis(10);
    let mut dev = I2c::open_first(&config)?;

    // Enable the level shifter that connects the MCP2221 to the I2C bus on the
    // HPS. This doesn't really belong in an "example" for this crate, but is
    // useful for HPS development.
    let mut gpio_config = GpioConfig::default();
    gpio_config.set_direction(0, mcp2221::Direction::Output);
    gpio_config.set_value(0, true);
    dev.configure_gpio(&gpio_config)?;

    dev.check_bus()?;

    println!("{}", dev.get_device_info()?);

    for base_address in (0..=127).step_by(16) {
        for offset in 0..=15 {
            let address = base_address + offset;
            match dev.read(address, &mut [0u8]) {
                Ok(_) => print!("0x{:02x}", address),
                Err(_) => print!(" -- "),
            }
        }
        println!();
    }

    Ok(())
}
