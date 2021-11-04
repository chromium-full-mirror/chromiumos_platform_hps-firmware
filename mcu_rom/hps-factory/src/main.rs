// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod hps_i2c;
mod image_workflow;
mod test_workflow;
mod write_stage0_workflow;

use anyhow::bail;
use anyhow::Result;
use clap::Clap;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use std::path::PathBuf;

const STAGE0_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stage0.bin"));
const APPLICATION_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/application.bin"));
const GATEWARE_BITSTREAM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hps_platform.bit"));
const SOC_ROM_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/soc_rom.bin"));
const SOC_ROM_START_ADDRESS: u32 = 2 * 1024 * 1024;

/// Runs HPS post-install factory workflow
#[derive(Clap, Debug, Clone)]
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

    /// whether to verify writes
    #[clap(long)]
    verify_writes: bool,

    /// directory into which to write captured images
    #[clap(long)]
    image_out_dir: Option<PathBuf>,

    /// assume that all firmware has already been written
    #[clap(long)]
    skip_firmware_writes: bool,

    /// whether to write stage0
    #[clap(long)]
    write_stage0: bool,
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
        return run_workflow(&mut i2c, config);
    }
    if let Some(dev_path) = &config.dev {
        let mut i2c = linux_embedded_hal::I2cdev::new(dev_path)?;
        return run_workflow(&mut i2c, config);
    }
    bail!("No supported device flag was specified");
}

pub(crate) fn run_workflow<E, I>(i2c: &mut I, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    if let Some(output_dir) = &config.image_out_dir {
        image_workflow::capture_images(i2c, output_dir, config.clone())
    } else if config.write_stage0 {
        write_stage0_workflow::run(i2c, config)
    } else {
        test_workflow::run(i2c)
    }
}
