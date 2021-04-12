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
use mcu_common::memory_banks;
use mcu_common::registers;

const HPS_ADDRESS: u8 = 0x30;

#[derive(FromArgs)]
/// Communicates with the HPS.
struct Flags {
    #[argh(option)]
    /// ftdi device serial number
    device: Option<String>,

    #[argh(switch)]
    /// list FTDI devices
    list_devices: bool,

    #[argh(option)]
    /// write memory bank
    write_bank: Option<u8>,

    #[argh(option)]
    /// write memory at the specified address
    write_address: Option<u32>,

    #[argh(option)]
    /// file from which to read MCU firmware
    mcu_firmware: Option<PathBuf>,

    #[argh(positional)]
    /// registers to read (reg) or write (reg=value), or values to write to
    /// memory
    values: Vec<String>,
}

struct HpsInterface<'a> {
    i2c: I2c<'a, Ft2232h>,
}

impl<'a> HpsInterface<'a> {
    fn new(ftdi: &'a Ft2232hHal<Initialized>) -> Result<Self> {
        let i2c = ftdi.i2c()?;
        let mut hps = Self { i2c };
        let magic = hps.read_register(registers::MAGIC)?;
        let expected = mcu_common::hps_magic_code();
        if magic != expected {
            bail!(
                "HPS not detected (expected {:x}, read {:x})",
                expected,
                magic
            );
        }
        Ok(hps)
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

    fn write_firmware(&mut self, input_filename: &Path) -> Result<()> {
        let bytes = std::fs::read(input_filename)?;
        let mut address = 0;
        for chunk in bytes.chunks(128) {
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

fn main() -> Result<()> {
    let flags: Flags = argh::from_env();
    if flags.list_devices {
        return print_device_list();
    }
    let ftdi = open_ftdi(&flags)?;
    let mut hps = HpsInterface::new(&ftdi)?;
    println!("HPS detected");
    if let Some(file) = &flags.mcu_firmware {
        return hps.write_firmware(file);
    }
    if let (Some(bank), Some(write_address)) = (&flags.write_bank, &flags.write_address) {
        let mut values = Vec::new();
        for value in &flags.values {
            values.push(value.parse::<u8>()?);
        }
        return hps.write_memory(*bank, *write_address, &values);
    }
    for reg in flags.values {
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
