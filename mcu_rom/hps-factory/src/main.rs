// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod test_workflow;

use anyhow::bail;
use anyhow::Result;
use clap::Clap;
use std::path::PathBuf;

/// Runs HPS post-install factory workflow
#[derive(Clap, Debug)]
#[clap(name = "config")]
struct Config {
    /// path to I2C device
    #[clap(long)]
    dev: Option<PathBuf>,

    /// connect to HPS via an MCP2221
    #[cfg(feature = "mcp2221")]
    #[clap(long)]
    mcp: bool,

    #[clap(long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let config = Config::parse();
    if config.verbose {
        simple_logger::SimpleLogger::new().init()?;
    }
    #[cfg(feature = "mcp2221")]
    if config.mcp {
        let mut i2c_config = mcp2221::I2cConfig::default();
        i2c_config.speed_hz = 400_000;
        let mut i2c = mcp2221::I2c::open_first(&i2c_config)?;
        test_workflow::run(&mut i2c)?;
        return Ok(());
    }
    if let Some(dev_path) = &config.dev {
        let mut i2c = linux_embedded_hal::I2cdev::new(dev_path)?;
        test_workflow::run(&mut i2c)?;
        return Ok(());
    }
    bail!("No supported device flag was specified");
}
