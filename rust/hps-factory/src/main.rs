// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod boot_loop;
mod boot_workflow;
mod erase_stage0_workflow;
mod hps_i2c;
#[cfg(feature = "image-transfer")]
mod image_workflow;
mod post_install_test_workflow;
mod test_workflow;
mod write_stage0_workflow;

use anyhow::Context;
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
const APPLICATION_RELEASE_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/application.release.bin"));
const ONE_TIME_INIT_LOADER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init_loader.bin"));

/// Runs HPS post-install factory workflow
#[derive(Clap, Debug, Clone)]
#[clap(name = "config")]
struct Config {
    /// path to I2C device
    #[clap(long, alias = "bus", default_value = "/dev/i2c-hps-controller")]
    dev: PathBuf,

    /// offset of GPIO pin that controls power to the HPS. Only GPIO chip 0 is
    /// supported. Set to the empty string to prevent power control.
    #[clap(long, default_value = "327")]
    power_gpio_offset: String,

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
    Factory(FactoryConfig),

    /// runs the test workflow
    #[clap(name = "test")]
    Test(TestConfig),

    /// write stage0 at the start of MCU flash
    #[clap(name = "write-stage0")]
    WriteStage0(WriteStage0Config),

    /// asks stage0 to erase the start of flash so that the system bootloader
    /// becomes active again after a power-cycle.
    #[clap(name = "erase-stage0")]
    EraseStage0,

    /// runs the boot sequence in a loop
    #[clap(name = "boot-loop")]
    BootLoop(BootLoopConfig),

    /// capture images from the device and write them to the filesystem.
    #[cfg(feature = "image-transfer")]
    #[clap(name = "capture-images")]
    CaptureImages(CaptureImagesConfig),

    /// runs all post-install tests
    #[clap(name = "post-install-test")]
    PostInstallTest(PostInstallTestConfig),

    /// writes release firmware and boots it. Requires stage0 to already be
    /// present
    #[clap(name = "boot")]
    Boot(BootConfig),
}

#[derive(Debug, Clap, Clone)]
struct BootConfig {
    /// assume that the FPGA bitstream is already up-to-date
    #[clap(long)]
    skip_bitstream_write: bool,
}

#[derive(Debug, Clap, Clone)]
struct PostInstallTestConfig {
    /// whether to skip writing MCU and SPI firmware
    #[clap(long)]
    skip_firmware_writes: bool,

    /// assume that the FPGA bitstream is already up-to-date
    #[clap(long)]
    skip_bitstream_write: bool,

    /// number of iterations of FPGA SPI flash read test to perform
    #[clap(long, default_value = "50")]
    fpga_spi_read_iterations: u16,

    /// number of I2C requests to make when checking I2C bus
    #[clap(long, default_value = "500")]
    host_i2c_iterations: u64,

    /// number of iterations of FPGA<->MCU communication test to perform
    #[clap(long, default_value = "50")]
    fpga_mcu_iterations: u16,

    /// number of iterations of MCU<->camera I2C test to perform
    #[clap(long, default_value = "50")]
    camera_i2c_iterations: u16,

    /// number of iterations of camera data bus test to perform
    #[clap(long, default_value = "50")]
    camera_data_bus_iterations: u16,
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

    /// assume that the FPGA bitstream is already up-to-date
    #[clap(long)]
    skip_bitstream_write: bool,

    /// whether to write raw files in addition to PNGs
    #[clap(long)]
    raw: bool,

    /// whether to write a latest.png symlink
    #[clap(long)]
    latest_symlink: bool,

    /// wait until the parent of the output directory exists
    #[clap(long)]
    wait_parent_dir_exists: bool,

    /// how long to wait with no images being received before failing
    #[clap(long)]
    timeout_seconds: Option<u64>,

    /// rotation to apply to images from the camera
    #[clap(long, default_value = "none")]
    rotation: application::Rotation,

    /// number of images to capture. 0 (default) for unlimited.
    #[clap(long, default_value = "0")]
    num_images: u64,
}

#[derive(Debug, Clap, Clone)]
struct TestConfig {
    /// skip checking SPI flash part number
    #[clap(long)]
    skip_spi_flash_id_check: bool,
}

#[derive(Debug, Clap, Clone)]
struct WriteStage0Config {
    /// whether to disable the boot0 pin
    #[clap(long)]
    disable_boot0: bool,

    /// whether to permanently lock stage0 read-only. Once done, this cannot be
    /// undone.
    #[clap(long)]
    permanent_lock: bool,
}

#[derive(Debug, Clap, Clone)]
struct FactoryConfig {
    /// whether to permanently lock stage0 read-only. Once done, this cannot be
    /// undone.
    #[clap(long)]
    permanent_lock: bool,
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
        let mut config = config;
        // Prevent power control via GPIO when using an MCP2221, since it's
        // unlikely to do what we want.
        config.power_gpio_offset.clear();
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
    let mut i2c = linux_embedded_hal::I2cdev::new(&config.dev)
        .with_context(|| format!("Couldn't open HPS I2C device '{}'", config.dev.display()))?;
    run_workflow(&mut i2c, config)
}

pub(crate) fn run_workflow<E, I>(i2c: &mut I, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    match &config.command {
        Command::BootLoop(boot_loop_config) => boot_loop::run(i2c, &config, boot_loop_config),
        Command::EraseStage0 => {
            erase_stage0_workflow::run(i2c, config)?;
            println!("Stage0 erased");
            Ok(())
        }
        #[cfg(feature = "image-transfer")]
        Command::CaptureImages(capture_images_config) => {
            image_workflow::capture_images(i2c, capture_images_config, &config)
        }
        Command::WriteStage0(write_stage0_config) => {
            write_stage0_workflow::Stage0Writer::run(i2c, &config, write_stage0_config)
        }
        Command::Test(test_config) => test_workflow::run(i2c, test_config, &config),
        Command::Factory(factory_config) => {
            test_workflow::run(
                i2c,
                &TestConfig {
                    skip_spi_flash_id_check: false,
                },
                &config,
            )?;
            write_stage0_workflow::Stage0Writer::run(
                i2c,
                &config,
                &WriteStage0Config {
                    disable_boot0: false,
                    permanent_lock: factory_config.permanent_lock,
                },
            )?;
            post_install_test_workflow::PostInstallTester::run(
                i2c,
                // The numbers of iterations used here are higher than the
                // defaults for the post-install-test subcommand, since we
                // assume this is running non-interactively, so it's OK to take
                // a little bit longer.
                &PostInstallTestConfig {
                    skip_firmware_writes: false,
                    skip_bitstream_write: false,
                    fpga_spi_read_iterations: 500,
                    host_i2c_iterations: 5000,
                    fpga_mcu_iterations: 500,
                    camera_i2c_iterations: 500,
                    camera_data_bus_iterations: 500,
                },
                &config,
            )?;
            Ok(())
        }
        Command::PostInstallTest(test_config) => {
            post_install_test_workflow::PostInstallTester::run(i2c, test_config, &config)
        }
        Command::Boot(boot_config) => boot_workflow::run(i2c, boot_config, &config),
    }
}
