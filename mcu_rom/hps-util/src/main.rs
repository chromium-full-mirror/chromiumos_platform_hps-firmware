// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use ftd2xx_embedded_hal::libftd2xx::DeviceTypeError;
use ftd2xx_embedded_hal::libftd2xx::FtStatus;
use ftd2xx_embedded_hal::Ft2232hHal;
use ftd2xx_embedded_hal::Initialized;
use mcu_common::commands;
use std::path::PathBuf;

use hps_util::Ft2232hHps;
use hps_util::SendI2c;
use hps_util::{print_device_list, query_status};

mod ipc;
use ipc::setup_ipc;

#[derive(FromArgs)]
/// Communicates with the HPS.
struct Flags {
    #[argh(subcommand)]
    sub_command: SubCommand,

    #[argh(option)]
    /// ftdi device serial number
    device: Option<String>,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum SubCommand {
    ListDevices(ListDevices),
    FlashMcu(FlashMcu),
    Launch(Launch),
    Reset(Reset),
    AccessRegisters(AccessRegisters),
    WriteMemoryBank(WriteMemoryBank),
    StressTest(StressTest),
    Status(Status),
    Daemon(Daemon),
}

#[derive(FromArgs, PartialEq, Debug)]
/// List FTDI devices
#[argh(subcommand, name = "list_devices")]
struct ListDevices {}

#[derive(FromArgs, PartialEq, Debug)]
/// Flash MCU application
#[argh(subcommand, name = "flash_mcu")]
struct FlashMcu {
    #[argh(option)]
    /// file from which to read MCU firmware
    file: PathBuf,
}

#[derive(FromArgs, PartialEq, Debug)]
/// Launch MCU application
#[argh(subcommand, name = "launch")]
struct Launch {}

#[derive(FromArgs, PartialEq, Debug)]
/// Launch MCU application
#[argh(subcommand, name = "reg")]
struct AccessRegisters {
    #[argh(positional)]
    /// registers to read (reg) or write (reg=value)
    values: Vec<String>,
}

#[derive(FromArgs, PartialEq, Debug)]
/// Reset MCU
#[argh(subcommand, name = "reset")]
struct Reset {}

#[derive(FromArgs, PartialEq, Debug)]
/// Query HPS status
#[argh(subcommand, name = "status")]
struct Status {}

#[derive(FromArgs, PartialEq, Debug)]
/// Write some values to an arbitrary memory bank
#[argh(subcommand, name = "write_memory_bank")]
struct WriteMemoryBank {
    #[argh(option)]
    /// memory bank to write
    bank: u8,

    #[argh(option)]
    /// address within the bank to start writing at
    address: u32,

    #[argh(positional)]
    /// values to write to the memory bank
    values: Vec<u8>,
}

#[derive(FromArgs, PartialEq, Debug)]
/// Stress test communication with the HPS
#[argh(subcommand, name = "stress_test")]
struct StressTest {
    #[argh(option)]
    /// number of iterations
    iterations: u32,
}

#[derive(FromArgs, PartialEq, Debug)]
/// Run HPS util in daemon mode.
#[argh(subcommand, name = "daemon")]
struct Daemon {}

fn open_ftdi(flags: &Flags) -> Result<Ft2232hHal<Initialized>> {
    let ft_hal;
    if let Some(serial_number) = &flags.device {
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
        Err(x) => Err(x)?,
    }
}

fn access_registers(hps: &mut Ft2232hHps, flags: &AccessRegisters) -> Result<()> {
    for reg in &flags.values {
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

fn stress_test(hps: &mut Ft2232hHps, config: &StressTest) -> Result<()> {
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

fn main() -> Result<()> {
    let flags: Flags = argh::from_env();
    if matches!(flags.sub_command, SubCommand::ListDevices(_)) {
        let v = print_device_list()?;
        println!("{:#?}", v);
    }
    let ftdi = Box::leak(Box::new(open_ftdi(&flags)?));
    let i2c = ftdi.i2c()?;
    // ftd2xx_embedded_hal::FtInner contains a `*mut c_void`, which isn't Send
    // by default, so we wrap it in a type that implements Send.
    //
    // TODO: Determine if it's really safe to send between threads and add the
    // appropriate Send implementation in the upstream library.
    let mut hps = Ft2232hHps::new(unsafe { SendI2c::new(i2c) })?;
    println!("HPS detected");
    match flags.sub_command {
        SubCommand::FlashMcu(FlashMcu { file }) => {
            let bytes = std::fs::read(file)?;
            hps.write_firmware(&bytes)?;
            println!("Sucessfully wrote {} bytes of firmware", bytes.len());
            Ok(())
        }
        SubCommand::Launch(_) => hps.perform_command(commands::LAUNCH),
        SubCommand::Reset(_) => hps.perform_command(commands::RESET),
        SubCommand::AccessRegisters(config) => access_registers(&mut hps, &config),
        SubCommand::WriteMemoryBank(config) => {
            hps.write_memory(config.bank, config.address, &config.values)
        }
        SubCommand::ListDevices(_) => unreachable!(),
        SubCommand::StressTest(config) => stress_test(&mut hps, &config),
        SubCommand::Status(_) => {
            let err_str = query_status(&mut hps)?;
            println!("{}", err_str);
            Ok(())
        }
        SubCommand::Daemon(_) => {
            setup_ipc(hps)?;
            Ok(())
        }
    }
}
