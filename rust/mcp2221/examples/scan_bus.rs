// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use embedded_hal::blocking::i2c::Read;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        println!("Error: {}", error);
    }
}

fn run() -> mcp2221::Result<()> {
    let mut config = mcp2221::Config::default();
    config.i2c_speed_hz = 400_000;
    config.timeout = Duration::from_millis(10);
    let mut dev = mcp2221::Handle::open_first(&config)?;

    // Enable the level shifter that connects the MCP2221 to the I2C bus on the
    // HPS. This doesn't really belong in an "example" for this crate, but is
    // useful for HPS development.
    let mut gpio_config = mcp2221::GpioConfig::default();
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
