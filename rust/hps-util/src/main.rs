// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use host_dev_common::kernel_driver;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use hps_interface::InterruptLine;
use hps_interface::NoopInterruptLine;
use hps_util::monitor;
use hps_util::print_status;
use interrupt_proxy::FtdiInterruptListener;
use mcu_common::commands::Command;
use mcu_common::registers::Register;
use std::path::PathBuf;

/// Which mechanism are we using to observe the interrupt line?
#[derive(clap::ArgEnum, Clone, PartialEq, Debug)]
enum InterruptInterface {
    None,
    LinuxGpio323,
    AvrFtdiProxy,
}

#[derive(Parser)]
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

    #[clap(long)]
    /// whether to reset MCP2221
    reset_mcp: bool,

    #[clap(long, arg_enum, default_value = "none")]
    /// which interface to use for observing the HPS→MLB interrupt line
    interrupt: InterruptInterface,
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
    /// Print scores and latency in a loop
    Monitor,
    /// Wait for HPS to assert its interrupt line
    #[clap(name = "wfi")]
    WaitForInterrupt,
}

#[derive(Parser, PartialEq, Debug)]
/// Flash MCU application
struct FlashMcu {
    #[clap(long)]
    /// file from which to read MCU firmware
    file: PathBuf,
}

#[derive(Parser, PartialEq, Debug)]
/// Flash FPGA bitstream and application to SPI flash
struct FlashSpi {
    #[clap(long)]
    /// file from which to read FPGA bitstream
    bitstream: PathBuf,
    #[clap(long)]
    /// file from which to read FPGA application ROM
    application: PathBuf,
}

#[derive(Parser, PartialEq, Debug)]
/// Launch stage1
struct Launch1 {}

#[derive(Parser, PartialEq, Debug)]
/// Erase stage0
struct EraseStage0 {}

#[derive(Parser, PartialEq, Debug)]
/// Erase stage1
struct EraseStage1 {}

#[derive(Parser, PartialEq, Debug)]
/// Launch application
struct LaunchApp {}

#[derive(Parser, PartialEq, Debug)]
/// Read or write arbitrary registers
struct AccessRegisters {
    /// registers to read (reg) or write (reg=value)
    values: Vec<String>,
}

#[derive(Parser, PartialEq, Debug)]
/// Reset MCU
struct Reset {}

#[derive(Parser, PartialEq, Debug)]
/// Query HPS status
struct Status {}

#[derive(Parser, PartialEq, Debug)]
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

#[derive(Parser, PartialEq, Debug)]
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
            println!(
                "Register {:?} (0x{:02x?}) = {:x?}",
                register, register as u8, bytes
            );
            continue;
        }
        if let Some((reg, value)) = parse_reg_write(reg) {
            hps.write_register(reg, value)?;
            println!("Wrote register {:?}={}", reg, value);
            continue;
        }
        if let Some(reg) = parse_reg(reg) {
            println!(
                "Register {:?} (0x{:02x?}) = {}",
                reg,
                reg as u8,
                hps.read_register(reg)?
            );
            continue;
        }
        bail!(
            "Expected REG or REG[READLEN] or REG=VALUE, but found `{}`",
            reg
        )
    }
    Ok(())
}

fn parse_long_reg_spec(spec: &str) -> Option<(Register, usize)> {
    let mut parts = spec.split('[');
    let register = parse_reg(parts.next()?)?;
    let length = parts.next()?.split(']').next()?.parse::<usize>().ok()?;
    Some((register, length))
}

fn parse_reg_write(s: &str) -> Option<(Register, u16)> {
    let mut parts = s.split('=');
    let register = parse_reg(parts.next()?)?;
    let value_str = parts.next()?;
    let value = if let Some(hex_str) = value_str.strip_prefix("0x") {
        u16::from_str_radix(hex_str, 16).ok()?
    } else {
        value_str.parse::<u16>().ok()?
    };
    Some((register, value))
}

fn parse_reg(s: &str) -> Option<Register> {
    s.parse::<u8>().ok()?.try_into().ok()
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
            fhps.write_register(Register::UserPresentStatus, v as u16)?;
        };
        return Ok(Box::new(fhps));
    }

    let interrupt: Box<dyn InterruptLine> = match flags.interrupt {
        InterruptInterface::None => Box::new(NoopInterruptLine {}),
        InterruptInterface::AvrFtdiProxy => Box::new(FtdiInterruptListener::open()?),
        InterruptInterface::LinuxGpio323 => {
            // gpiochip0 line 323 is the HPS→MLB interrupt line on Taeko.
            // Currently we have no good way to look this up in ACPI, so just hardcode it for now.
            let line = gpio_cdev::Chip::new("/dev/gpiochip0")
                .context("Failed to open GPIO chip")?
                .get_line(323)
                .context("Failed to look up GPIO line")?;
            let handle = line
                .request(
                    gpio_cdev::LineRequestFlags::INPUT,
                    /* default, unused for input */ 0,
                    "hps-util",
                )
                .context("Failed to request GPIO line")?;
            Box::new(handle)
        }
    };

    #[cfg(feature = "mcp2221")]
    if flags.mcp {
        let mut config = mcp2221::Config::default();
        config.reset_on_open = flags.reset_mcp;
        config.i2c_speed_hz = 400_000;
        let mut dev = mcp2221::Handle::open_first(&config)
            .map_err(|error| anyhow::anyhow!("Failed to open HPS via I2C: {}", error))?;

        // Enable the level shifter that connects the MCP2221 to the HPS.
        let mut gpio_config = mcp2221::GpioConfig::default();
        gpio_config.set_direction(0, mcp2221::Direction::Output);
        gpio_config.set_value(0, true);
        dev.configure_gpio(&gpio_config)?;

        return Ok(Box::new(HpsAdapter::new(Box::new(dev), interrupt)?));
    }

    if kernel_driver::power_state() == kernel_driver::PowerState::Suspended {
        bail!("Device currently suspended by kernel driver. Try unbind command first.");
    }
    let i2c = linux_embedded_hal::I2cdev::new(&flags.dev)
        .with_context(|| format!("Couldn't open HPS I2C device '{}'", flags.dev.display()))?;
    Ok(Box::new(HpsAdapter::new(Box::new(i2c), interrupt)?))
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
        SubCommand::Monitor => monitor::run(
            &mut *hps,
            /* use_interrupt */ flags.interrupt != InterruptInterface::None,
        ),
        SubCommand::WaitForInterrupt => hps.wait_for_interrupt(),
    }
}

#[cfg(test)]
mod tests {
    use mcu_common::registers::Register;

    use super::parse_reg_write;

    #[test]
    fn test_parse_reg_write() {
        assert_eq!(parse_reg_write("3=12"), Some((Register::Command, 12)));
        assert_eq!(parse_reg_write("3=0xA"), Some((Register::Command, 10)));
        assert_eq!(parse_reg_write("3=0xa"), Some((Register::Command, 10)));
    }
}
