// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use clap::Clap;
use clap::Subcommand;
#[cfg(feature = "ftdi")]
use ftd2xx_embedded_hal::libftd2xx::DeviceTypeError;
#[cfg(feature = "ftdi")]
use ftd2xx_embedded_hal::libftd2xx::FtStatus;
#[cfg(feature = "ftdi")]
use ftd2xx_embedded_hal::Ft2232hHal;
#[cfg(feature = "ftdi")]
use ftd2xx_embedded_hal::Initialized;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
#[cfg(feature = "ftdi")]
use hps_util::SendI2c;
use mcu_common::commands::Command;
use mcu_common::registers;
use std::path::PathBuf;

#[cfg(feature = "ftdi")]
use hps_util::print_device_list;
use hps_util::query_status;

#[cfg(feature = "daemon")]
mod ipc;

#[derive(Clap)]
/// Communicates with the HPS.
struct Flags {
    #[clap(subcommand)]
    sub_command: SubCommand,

    #[cfg(feature = "ftdi")]
    #[clap(long)]
    /// open HPS via an FT2232H adapter
    ftdi: bool,

    #[cfg(feature = "ftdi")]
    #[clap(long)]
    /// ftdi device serial number
    ftdi_serial: Option<String>,

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
    #[cfg(feature = "ftdi")]
    #[clap(name = "list_devices")]
    ListDevices(ListDevices),
    #[clap(name = "flash_mcu")]
    FlashMcu(FlashMcu),
    Launch1(Launch1),
    #[clap(name = "launchapp")]
    LaunchApp(LaunchApp),
    Reset(Reset),
    #[clap(name = "erase_stage0")]
    EraseStage0(EraseStage0),
    #[clap(name = "reg")]
    AccessRegisters(AccessRegisters),
    #[clap(name = "write_memory_bank")]
    WriteMemoryBank(WriteMemoryBank),
    #[clap(name = "stress_test")]
    StressTest(StressTest),
    Status(Status),
    #[cfg(feature = "daemon")]
    Daemon(Daemon),
}

#[cfg(feature = "ftdi")]
#[derive(Clap, PartialEq, Debug)]
/// List FTDI devices
struct ListDevices {}

#[derive(Clap, PartialEq, Debug)]
/// Flash MCU application
struct FlashMcu {
    #[clap(long)]
    /// file from which to read MCU firmware
    file: PathBuf,
}

#[derive(Clap, PartialEq, Debug)]
/// Launch stage1
struct Launch1 {}

#[derive(Clap, PartialEq, Debug)]
/// Erase stage0
struct EraseStage0 {}

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

#[cfg(feature = "daemon")]
#[derive(Clap, PartialEq, Debug)]
/// Run HPS util in daemon mode.
struct Daemon {}

#[cfg(feature = "ftdi")]
fn open_ftdi(flags: &Flags) -> Result<Ft2232hHal<Initialized>> {
    let ft_hal;
    if let Some(serial_number) = &flags.ftdi_serial {
        ft_hal = Ft2232hHal::with_serial_number(serial_number);
    } else {
        ft_hal = Ft2232hHal::new();
    }
    match ft_hal {
        Ok(x) => Ok(x.init_default()?),
        Err(DeviceTypeError::FtStatus(FtStatus::DEVICE_NOT_OPENED)) => {
            // libftdi2 has a known issue that the ftdi_sio module interferes
            // with its operation, so we provide a more helpful message for that
            // case.
            bail!(
                "Failed to open FT device. Please run `sudo rmmod ftdi_sio` then try again.\n\
                   To make this permanent, run:\n\
                   echo \"blacklist ftdi_sio\" | sudo tee /etc/modprobe.d/local-dontload.conf"
            );
        }
        Err(x) => Err(x.into()),
    }
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
    if flags.mcp {
        let mut config = mcp2221::I2cConfig::default();
        config.speed_hz = 400_000;
        let mut dev = mcp2221::I2c::open_first(&config)
            .map_err(|error| anyhow!("Failed to open HPS via I2C: {}", error))?;

        // Enable the level shifter that connects the MCP2221 to the HPS.
        let mut gpio_config = mcp2221::GpioConfig::default();
        gpio_config.set_direction(0, mcp2221::Direction::Output);
        gpio_config.set_value(0, true);
        dev.configure_gpio(&gpio_config)?;

        return Ok(Box::new(HpsAdapter::new(dev)?));
    }
    #[cfg(feature = "ftdi")]
    if flags.ftdi {
        let ftdi = Box::leak(Box::new(open_ftdi(flags)?));
        let i2c = ftdi.i2c()?;
        // ftd2xx_embedded_hal::FtInner contains a `*mut c_void`, which isn't Send
        // by default, so we wrap it in a type that implements Send.
        //
        // TODO: Determine if it's really safe to send between threads and add the
        // appropriate Send implementation in the upstream library.
        let hps = HpsAdapter::new(unsafe { SendI2c::new(i2c) })?;
        return Ok(Box::new(hps));
    }
    bail!("No method of connecting to HPS specified, try --mcp");
}

fn main() -> Result<()> {
    let flags: Flags = Flags::parse();
    #[cfg(feature = "ftdi")]
    if matches!(flags.sub_command, SubCommand::ListDevices(_)) {
        let v = print_device_list()?;
        println!("{:#?}", v);
        return Ok(());
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
        SubCommand::Launch1(_) => hps.perform_command(Command::Launch1),
        SubCommand::LaunchApp(_) => hps.perform_command(Command::LaunchApp),
        SubCommand::Reset(_) => hps.perform_command(Command::Reset),
        SubCommand::EraseStage0(_) => hps.perform_command(Command::EraseStage0Start),
        SubCommand::AccessRegisters(config) => access_registers(&mut *hps, &config),
        SubCommand::WriteMemoryBank(config) => {
            hps.write_memory(config.bank, config.address, &config.values)
        }
        #[cfg(feature = "ftdi")]
        SubCommand::ListDevices(_) => unreachable!(),
        SubCommand::StressTest(config) => stress_test(&mut *hps, &config),
        SubCommand::Status(_) => {
            println!("Status register: {:?}", hps.status()?);
            println!("{}", query_status(&mut *hps)?);
            Ok(())
        }
        #[cfg(feature = "daemon")]
        SubCommand::Daemon(_) => {
            ipc::setup_ipc(hps)?;
            Ok(())
        }
    }
}
