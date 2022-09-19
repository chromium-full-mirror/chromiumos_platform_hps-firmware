// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod boot_workflow;
mod erase_stage0_workflow;
mod firmware;
mod hps_i2c;
#[cfg(feature = "image-transfer")]
mod image_workflow;
mod post_install_test_workflow;
mod print_part_ids_workflow;
mod test_stage0_workflow;
mod test_stage1_workflow;
mod write_stage0_workflow;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use host_dev_common::KernelDriverUnbinder;
use log::LevelFilter;
use std::path::PathBuf;

/// Runs HPS post-install factory workflow
#[derive(Parser, Debug, Clone)]
#[clap(name = "hps-factory")]
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

    /// connect to HPS via unix socket
    #[clap(long)]
    socket: Option<String>,

    #[clap(long)]
    verbose: bool,

    /// Disable logging
    #[clap(long)]
    quiet: bool,

    /// whether to verify writes
    #[clap(long)]
    verify_writes: bool,

    #[clap(subcommand)]
    command: Command,

    /// path FPGA bitstream. Overrides built-in bitstream
    #[clap(long)]
    fpga_bitstream: Option<PathBuf>,

    /// path FPGA ROM. Overrides built-in ROM
    #[clap(long)]
    fpga_rom: Option<PathBuf>,

    /// path stage0 ROM. Overrides built-in ROM
    #[clap(long)]
    stage0_rom: Option<PathBuf>,

    /// path MCU ROM. Overrides built-in ROM
    #[clap(long)]
    mcu_rom: Option<PathBuf>,

    /// whether to force writing firmware, even if it appears to be up-to-date
    #[clap(long)]
    force_firmware_update: bool,

    #[clap(long)]
    /// whether to reset MCP2221
    reset_mcp: bool,
}

#[derive(Debug, Subcommand, Clone)]
enum Command {
    /// runs the factory workflow
    #[clap(name = "factory")]
    Factory(FactoryConfig),

    /// write stage0 at the start of MCU flash
    #[clap(name = "write-stage0")]
    WriteStage0(WriteStage0Config),

    /// asks stage0 to erase the start of flash so that the system bootloader
    /// becomes active again after a power-cycle.
    #[clap(name = "erase-stage0")]
    EraseStage0,

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

    /// prints IDs of parts contained within the HPS
    PrintPartIds,

    /// tries various things with stage0 to make sure it behaves as expected
    TestStage0,

    /// tries various things with stage1 to make sure it behaves as expected
    TestStage1,

    /// writes all embedded firmware files to the filesystem
    WriteEmbeddedFirmware(WriteEmbeddedFirmwareConfig),
}

#[derive(Debug, Parser, Clone)]
struct BootConfig {
    /// assume that the FPGA bitstream is already up-to-date
    #[clap(long)]
    skip_bitstream_write: bool,
}

#[derive(Debug, Parser, Clone)]
struct WriteEmbeddedFirmwareConfig {
    /// directory into which to write firmware files
    #[clap(long)]
    out_dir: PathBuf,
}

#[derive(Debug, Parser, Clone)]
struct PostInstallTestConfig {
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
#[derive(Debug, Parser, Clone)]
struct CaptureImagesConfig {
    /// directory into which to write captured images
    #[clap(long)]
    out_dir: PathBuf,

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

#[derive(Debug, Parser, Clone)]
struct WriteStage0Config {
    /// whether to disable the boot0 pin
    #[clap(long)]
    disable_boot0: bool,

    /// whether to permanently lock stage0 read-only. Once done, this cannot be
    /// undone.
    #[clap(long)]
    permanent_lock: bool,

    /// whether to skip writing if there's an existing stage0 with the same
    /// reported version number as what we would write
    #[clap(long)]
    check_version: bool,

    /// skip all post-write initialization of option bytes
    #[clap(long, conflicts_with = "permanent-lock")]
    skip_one_time_init: bool,

    /// leave device at RDP level 0
    #[clap(long, conflicts_with = "permanent-lock")]
    rdp0: bool,
}

#[derive(Debug, Parser, Clone)]
struct FactoryConfig {
    /// whether to permanently lock stage0 read-only. Once done, this cannot be
    /// undone.
    #[clap(long)]
    permanent_lock: bool,
}

impl Config {
    fn log_level(&self) -> LevelFilter {
        if self.quiet {
            return LevelFilter::Error;
        }
        if self.verbose {
            return LevelFilter::Debug;
        }
        LevelFilter::Info
    }
}

fn main() -> Result<()> {
    let config = Config::parse();
    simple_logger::SimpleLogger::new()
        .with_level(config.log_level())
        .init()?;
    if let Command::WriteEmbeddedFirmware(write_config) = &config.command {
        return config.write_firmware_files(write_config);
    }
    #[cfg(feature = "mcp2221")]
    if config.mcp {
        let mut config = config;
        // Prevent power control via GPIO when using an MCP2221, since it's
        // unlikely to do what we want.
        config.power_gpio_offset.clear();
        let mut i2c_config = mcp2221::Config::default();
        i2c_config.reset_on_open = config.reset_mcp;
        i2c_config.i2c_speed_hz = 400_000;
        i2c_config.timeout = std::time::Duration::from_millis(500);
        // This timeout needs to be shorter than the STARTUP_TIMEOUT in
        // hps_i2c.rs. If both are 1 second, then we'll send a request which the
        // device won't see because it isn't ready, we'll then wait 1 second and
        // then give up.
        i2c_config.timeout = std::time::Duration::from_millis(100);
        let mut i2c = mcp2221::Handle::open_first(&i2c_config)?;
        i2c.check_bus()?;
        return run_workflow(&mut i2c, config);
    }
    if let Some(socket) = &config.socket {
        let mut i2c = unix_socket::I2cOverSocket::connect(&socket)?;
        return run_workflow(&mut i2c, config);
    }

    if is_hpsd_running().unwrap_or(false) {
        bail!("HPSD seems to be running. Please run `stop hpsd` before using hps-factory.");
    }
    // Since the HPS kernel driver takes exclusive ownership of the HPS I2C
    // address, we must unbind the driver while this tool is running. The driver
    // is bound again when _unbinder goes out of scope.
    let _unbinder = KernelDriverUnbinder::unbind()?;
    let mut i2c = linux_embedded_hal::I2cdev::new(&config.dev)
        .with_context(|| format!("Couldn't open HPS I2C device '{}'", config.dev.display()))?;
    run_workflow(&mut i2c, config)
}

/// Returns whether HPSD is running.
fn is_hpsd_running() -> Result<bool> {
    Ok(std::process::Command::new("pidof")
        .arg("hpsd")
        .output()?
        .status
        .success())
}

pub(crate) fn run_workflow<E, I>(i2c: &mut I, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    match &config.command {
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
        Command::Factory(factory_config) => {
            write_stage0_workflow::Stage0Writer::run(
                i2c,
                &config,
                &WriteStage0Config {
                    disable_boot0: false,
                    permanent_lock: factory_config.permanent_lock,
                    check_version: false,
                    skip_one_time_init: false,
                    rdp0: !factory_config.permanent_lock,
                },
            )?;
            post_install_test_workflow::PostInstallTester::run(
                i2c,
                &PostInstallTestConfig {
                    fpga_spi_read_iterations: 50,
                    host_i2c_iterations: 500,
                    fpga_mcu_iterations: 50,
                    camera_i2c_iterations: 50,
                    camera_data_bus_iterations: 50,
                },
                &config,
            )?;
            Ok(())
        }
        Command::PostInstallTest(test_config) => {
            post_install_test_workflow::PostInstallTester::run(i2c, test_config, &config)
        }
        Command::Boot(boot_config) => boot_workflow::run(i2c, boot_config, &config),
        Command::PrintPartIds => print_part_ids_workflow::run(i2c, &config),
        Command::TestStage0 => test_stage0_workflow::Stage0Tester::run(i2c, &config),
        Command::TestStage1 => test_stage1_workflow::Stage1Tester::run(i2c, &config),
        Command::WriteEmbeddedFirmware(_) => unreachable!(),
    }
}
