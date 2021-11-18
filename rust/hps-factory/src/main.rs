// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod boot_loop;
mod erase_stage0_workflow;
mod hps_i2c;
#[cfg(feature = "image-transfer")]
mod image_workflow;
mod one_time_init_workflow;
mod test_workflow;
mod write_stage0_workflow;

use anyhow::bail;
use anyhow::Result;
use clap::Clap;
use clap::Subcommand;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use std::path::PathBuf;

const STAGE0_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stage0.bin"));
const STAGE0_SYS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stage0.sys.bin"));
const APPLICATION_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/application.bin"));

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

    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand, Clone)]
enum Command {
    /// runs the factory workflow
    #[clap(name = "factory")]
    Factory,

    /// write stage0 at the start of MCU flash
    #[clap(name = "write_stage0")]
    WriteStage0,

    /// asks stage0 to erase the start of flash so that the system bootloader
    /// becomes active again after a power-cycle.
    #[clap(name = "erase_stage0")]
    EraseStage0,

    /// performs one-time initialization of the HPS module
    #[clap(name = "one_time_init")]
    OneTimeInit,

    /// runs the boot sequence in a loop
    #[clap(name = "boot_loop")]
    BootLoop(BootLoopConfig),

    /// capture images from the device and write them to the filesystem.
    #[cfg(feature = "image-transfer")]
    #[clap(name = "capture_images")]
    CaptureImages(CaptureImagesConfig),
}

#[cfg(feature = "image-transfer")]
#[derive(Debug, Clap, Clone)]
struct CaptureImagesConfig {
    /// directory into which to write captured images
    #[clap(long)]
    out_dir: PathBuf,

    /// assume that all firmware has already been written
    #[clap(long)]
    skip_firmware_writes: bool,

    /// whether to write raw files in addition to PNGs
    #[clap(long)]
    raw: bool,

    /// whether to write a latest.png symlink
    #[clap(long)]
    latest_symlink: bool,

    /// wait until the output directory exists
    #[clap(long)]
    wait_dir_exists: bool,
}

#[derive(Debug, Clap, Clone)]
struct BootLoopConfig {
    /// assume that all firmware has already been written
    #[clap(long)]
    skip_firmware_writes: bool,
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
        // This timeout needs to be shorter than the STARTUP_TIMEOUT in
        // hps_i2c.rs. If both are 1 second, then we'll send a request which the
        // device won't see because it isn't ready, we'll then wait 1 second and
        // then give up.
        i2c_config.timeout = std::time::Duration::from_millis(100);
        let mut i2c = mcp2221::I2c::open_first(&i2c_config)?;
        i2c.check_bus()?;
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
    match &config.command {
        Command::BootLoop(boot_loop_config) => boot_loop::run(i2c, &config, &boot_loop_config),
        Command::OneTimeInit => one_time_init_workflow::run(i2c, config),
        Command::EraseStage0 => erase_stage0_workflow::run(i2c, config),
        #[cfg(feature = "image-transfer")]
        Command::CaptureImages(capture_images_config) => {
            image_workflow::capture_images(i2c, &capture_images_config, &config)
        }
        Command::WriteStage0 => write_stage0_workflow::run(i2c, config),
        Command::Factory => test_workflow::run(i2c),
    }
}
