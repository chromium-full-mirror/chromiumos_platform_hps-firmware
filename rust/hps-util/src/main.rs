// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Clap;
use clap::Subcommand;
use host_dev_common::kernel_driver;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use hps_util::monitor;
use hps_util::print_status;
use mcu_common::commands::Command;
use mcu_common::registers;
use std::path::PathBuf;

#[derive(Clap)]
/// Communicates with the HPS.
struct Flags {
    #[clap(subcommand)]
    sub_command: SubCommand,

    #[clap(long, alias = "bus", default_value = "/dev/i2c-hps-controller")]
    /// path to a linux I2C device
    dev: PathBuf,

    #[cfg(feature = "mcp2221")]
    #[clap(long)]
    /// open HPS via an MCP2221 I2C adapter
    mcp: bool,

    #[clap(long)]
    /// whether to use a fake HPS device instead of trying to open a real one
    fake: bool,

    #[clap(long)]
    /// A "fake" value for the register USER_PRESENT.
    fake_reg: Option<String>,
}

#[derive(Subcommand, PartialEq, Debug)]
enum SubCommand {
    FlashMcu(FlashMcu),
    FlashSpi(FlashSpi),
    Launch1(Launch1),
    #[clap(name = "launchapp")]
    LaunchApp(LaunchApp),
    Reset(Reset),
    EraseStage0(EraseStage0),
    EraseStage1(EraseStage1),
    #[clap(name = "reg")]
    AccessRegisters(AccessRegisters),
    WriteMemoryBank(WriteMemoryBank),
    StressTest(StressTest),
    Status(Status),
    Unbind,
    Bind,
    Monitor,
}

#[derive(Clap, PartialEq, Debug)]
/// Flash MCU application
struct FlashMcu {
    #[clap(long)]
    /// file from which to read MCU firmware
    file: PathBuf,
}

#[derive(Clap, PartialEq, Debug)]
/// Flash FPGA bitstream and application to SPI flash
struct FlashSpi {
    #[clap(long)]
    /// file from which to read FPGA bitstream
    bitstream: PathBuf,
    #[clap(long)]
    /// file from which to read FPGA application ROM
    application: PathBuf,
}

#[derive(Clap, PartialEq, Debug)]
/// Launch stage1
struct Launch1 {}

#[derive(Clap, PartialEq, Debug)]
/// Erase stage0
struct EraseStage0 {}

#[derive(Clap, PartialEq, Debug)]
/// Erase stage1
struct EraseStage1 {}

#[derive(Clap, PartialEq, Debug)]
/// Launch application
struct LaunchApp {}

#[derive(Clap, PartialEq, Debug)]
/// Read or write arbitrary registers
struct AccessRegisters {
    /// registers to read (reg) or write (reg=value)
    values: Vec<String>,
}

#[derive(Clap, PartialEq, Debug)]
/// Reset MCU
struct Reset {}

#[derive(Clap, PartialEq, Debug)]
/// Query HPS status
struct Status {}

#[derive(Clap, PartialEq, Debug)]
/// Write some values to an arbitrary memory bank
struct WriteMemoryBank {
    #[clap(long)]
    /// memory bank to write
    bank: u8,

    #[clap(long)]
    /// address within the bank to start writing at
    address: u32,

    /// values to write to the memory bank
    values: Vec<u8>,
}

#[derive(Clap, PartialEq, Debug)]
/// Stress test communication with the HPS
struct StressTest {
    #[clap(long)]
    /// number of iterations
    iterations: u32,
}

fn access_registers(hps: &mut dyn Hps, flags: &AccessRegisters) -> Result<()> {
    for reg in &flags.values {
        if let Some((register, length)) = parse_long_reg_spec(reg) {
            let bytes = hps.read_register_bytes(register, length)?;
            println!("Register {} = {:x?}", register, bytes);
            continue;
        }
        let mut parts = reg.split('=');
        match (
            parts.next().map(|part| part.parse::<u8>()),
            parts.next().map(|part| part.parse::<u16>()),
        ) {
            (Some(Ok(reg)), None) => println!("Register {} = {}", reg, hps.read_register(reg)?),
            (Some(Ok(reg)), Some(Ok(value))) => {
                hps.write_register(reg, value)?;
                println!("Wrote register {}={}", reg, value)
            }
            _ => {
                bail!("Expected u8 or u8=16, but found `{}`", reg)
            }
        }
    }
    Ok(())
}

fn parse_long_reg_spec(spec: &str) -> Option<(u8, usize)> {
    let mut parts = spec.split('[');
    let register = parts.next()?.parse::<u8>().ok()?;
    let length = parts.next()?.split(']').next()?.parse::<usize>().ok()?;
    Some((register, length))
}

fn stress_test(hps: &mut dyn Hps, config: &StressTest) -> Result<()> {
    let mut errors = 0;
    for _ in 0..config.iterations {
        if hps.check_magic().is_err() {
            errors += 1;
        }
    }
    println!(
        "Got errors on {} out of {} reads",
        errors, config.iterations
    );
    Ok(())
}

fn open_device(flags: &Flags) -> Result<Box<dyn Hps + Send>> {
    if flags.fake {
        let mut fhps = hps_util::FakeHps::default();
        // TODO(quasisec): Allow parsing a slice of tuples of (reg, val) pairs.
        if let Some(reg) = &flags.fake_reg {
            let v = reg.parse::<i16>()?;
            fhps.write_register(registers::USER_PRESENT_STATUS, v as u16)?;
        };
        return Ok(Box::new(fhps));
    }
    #[cfg(feature = "mcp2221")]
    if flags.mcp {
        let mut config = mcp2221::Config::default();
        config.i2c_speed_hz = 400_000;
        let mut dev = mcp2221::Handle::open_first(&config)
            .map_err(|error| anyhow::anyhow!("Failed to open HPS via I2C: {}", error))?;

        // Enable the level shifter that connects the MCP2221 to the HPS.
        let mut gpio_config = mcp2221::GpioConfig::default();
        gpio_config.set_direction(0, mcp2221::Direction::Output);
        gpio_config.set_value(0, true);
        dev.configure_gpio(&gpio_config)?;

        return Ok(Box::new(HpsAdapter::new(dev)?));
    }
    if kernel_driver::power_state() == kernel_driver::PowerState::Suspended {
        bail!("Device currently suspended by kernel driver. Try unbind command first.");
    }
    let i2c = linux_embedded_hal::I2cdev::new(&flags.dev)
        .with_context(|| format!("Couldn't open HPS I2C device '{}'", flags.dev.display()))?;
    Ok(Box::new(HpsAdapter::new(i2c)?))
}

fn main() -> Result<()> {
    let flags: Flags = Flags::parse();
    match flags.sub_command {
        SubCommand::Unbind => return kernel_driver::unbind(),
        SubCommand::Bind => return kernel_driver::bind(),
        _ => {}
    }
    let mut hps = open_device(&flags)?;
    println!("HPS detected");
    match flags.sub_command {
        SubCommand::FlashMcu(FlashMcu { file }) => {
            let bytes = std::fs::read(file)?;
            hps.write_firmware(&bytes)?;
            println!("Sucessfully wrote {} bytes of firmware", bytes.len());
            Ok(())
        }
        SubCommand::FlashSpi(FlashSpi {
            bitstream,
            application,
        }) => {
            let bitstream_bytes = std::fs::read(bitstream)?;
            let app_bytes = std::fs::read(application)?;
            hps.write_spi_flash(&bitstream_bytes, &app_bytes)?;
            println!(
                "Sucessfully wrote {} bytes (bitstream) and {} bytes (application)",
                bitstream_bytes.len(),
                app_bytes.len()
            );
            Ok(())
        }
        SubCommand::Launch1(_) => hps.perform_command(Command::Launch1),
        SubCommand::LaunchApp(_) => hps.perform_command(Command::LaunchApp),
        SubCommand::Reset(_) => hps.perform_command(Command::Reset),
        SubCommand::EraseStage0(_) => hps.perform_command(Command::EraseStage0Start),
        SubCommand::EraseStage1(_) => hps.perform_command(Command::EraseStage1),
        SubCommand::AccessRegisters(config) => access_registers(&mut *hps, &config),
        SubCommand::WriteMemoryBank(config) => {
            hps.write_memory(config.bank, config.address, &config.values)
        }
        SubCommand::StressTest(config) => stress_test(&mut *hps, &config),
        SubCommand::Status(_) => print_status(&mut *hps),
        SubCommand::Unbind => unreachable!(),
        SubCommand::Bind => unreachable!(),
        SubCommand::Monitor => monitor::run(&mut *hps),
    }
}
