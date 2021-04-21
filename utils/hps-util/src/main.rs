// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;
use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use embedded_hal::prelude::*;
use ftd2xx_embedded_hal::libftd2xx;
use ftd2xx_embedded_hal::libftd2xx::DeviceTypeError;
use ftd2xx_embedded_hal::libftd2xx::Ft2232h;
use ftd2xx_embedded_hal::libftd2xx::FtStatus;
use ftd2xx_embedded_hal::Ft2232hHal;
use ftd2xx_embedded_hal::I2c;
use ftd2xx_embedded_hal::Initialized;
use mcu_common::commands;
use mcu_common::memory_banks;
use mcu_common::registers;
use std::time::Duration;

const HPS_ADDRESS: u8 = 0x30;

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

struct HpsInterface<'a> {
    i2c: I2c<'a, Ft2232h>,
}

impl<'a> HpsInterface<'a> {
    fn new(ftdi: &'a Ft2232hHal<Initialized>) -> Result<Self> {
        let i2c = ftdi.i2c()?;
        let mut hps = Self { i2c };
        hps.check_magic()?;
        Ok(hps)
    }

    /// Reads the "magic" register which always contains a particular value if
    /// the device is a HPS. Returns an error on communication failure or if the
    /// returned value isn't what's expected.
    fn check_magic(&mut self) -> Result<()> {
        let magic = self.read_register(registers::MAGIC)?;
        let expected = mcu_common::hps_magic_code();
        if magic != expected {
            bail!(
                "HPS not detected (expected {:x}, read {:x})",
                expected,
                magic
            );
        }
        Ok(())
    }

    fn read_register(&mut self, reg: u8) -> Result<u16> {
        let mut buffer = [0u8; 2];
        self.i2c
            .write_read(HPS_ADDRESS, &[register_access_command(reg)], &mut buffer)?;
        Ok(((buffer[0] as u16) << 8) | (buffer[1] as u16))
    }

    fn write_register(&mut self, reg: u8, value: u16) -> Result<()> {
        let buffer = [
            register_access_command(reg),
            (value >> 8) as u8,
            (value & 0xff) as u8,
        ];
        Ok(self.i2c.write(HPS_ADDRESS, &buffer)?)
    }

    fn perform_command(&mut self, command: u16) -> Result<()> {
        self.write_register(registers::COMMAND, command)
    }

    fn write_memory(&mut self, bank: u8, address: u32, values: &[u8]) -> Result<()> {
        if bank >= 64 {
            bail!("Invalid memory bank {}", bank);
        }
        let mut buffer = Vec::with_capacity(1 + std::mem::size_of::<u32>() + values.len());
        buffer.push(bank);
        buffer.extend_from_slice(&address.to_be_bytes());
        buffer.extend_from_slice(values);
        Ok(self.i2c.write(HPS_ADDRESS, &buffer)?)
    }

    fn wait_memory_bank_available(&mut self, memory_bank: u8) -> Result<()> {
        for _ in 0..100 {
            if let Ok(available) = self.read_register(registers::MEMORY_BANK_AVAILABLE) {
                if available & (1 << memory_bank) != 0 {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        bail!(
            "Timeout while waiting for memory bank {} to become available",
            memory_bank
        );
    }

    fn write_firmware(&mut self, input_filename: &Path) -> Result<()> {
        let bytes = std::fs::read(input_filename)?;
        let mut address = 0;
        for chunk in bytes.chunks(256) {
            self.wait_memory_bank_available(memory_banks::MCU_RW)?;
            self.write_memory(memory_banks::MCU_RW, address, chunk)?;
            address += chunk.len() as u32;
        }
        println!("Sucessfully wrote {} bytes of firmware", bytes.len());
        Ok(())
    }
}

fn register_access_command(reg: u8) -> u8 {
    reg | 0x80
}

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

fn print_device_list() -> Result<()> {
    for device in libftd2xx::list_devices()? {
        println!("{}: {}", device.serial_number, device.description);
    }
    Ok(())
}

fn access_registers(hps: &mut HpsInterface, flags: &AccessRegisters) -> Result<()> {
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

fn stress_test(hps: &mut HpsInterface, config: &StressTest) -> Result<()> {
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
        return print_device_list();
    }
    let ftdi = open_ftdi(&flags)?;
    let mut hps = HpsInterface::new(&ftdi)?;
    println!("HPS detected");
    match flags.sub_command {
        SubCommand::FlashMcu(FlashMcu { file }) => hps.write_firmware(&file),
        SubCommand::Launch(_) => hps.perform_command(commands::LAUNCH),
        SubCommand::Reset(_) => hps.perform_command(commands::RESET),
        SubCommand::AccessRegisters(config) => access_registers(&mut hps, &config),
        SubCommand::WriteMemoryBank(config) => {
            hps.write_memory(config.bank, config.address, &config.values)
        }
        SubCommand::ListDevices(_) => unreachable!(),
        SubCommand::StressTest(config) => stress_test(&mut hps, &config),
    }
}
