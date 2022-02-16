// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod fake_hps;
mod send_i2c;

use anyhow::Result;
pub use fake_hps::FakeHps;
use hps_interface::Hps;
use mcu_common::registers;
use mcu_common::OptionBytesConfigRequest;
pub use send_i2c::SendI2c;

pub fn print_status(hps: &mut dyn Hps) -> Result<()> {
    let stage = hps.stage()?;
    println!("Stage: {:?}", stage);
    println!("Status register: {:?}", hps.status()?);
    match hps.check_errors() {
        Ok(_) => println!("No errors reported"),
        Err(error) => println!("{}", error),
    }
    match stage {
        hps_interface::Stage::Unknown => {}
        hps_interface::Stage::Stage0 => {
            println!(
                "Stage0 version: 0x{:x}",
                hps.read_register(registers::HARDWARE_VERSION)?
            );
            println!(
                "Stage1 version: 0x{:x}",
                ((hps.read_register(registers::FIRMWARE_VERSION_HIGH)? as u32) << 16)
                    | hps.read_register(registers::FIRMWARE_VERSION_LOW)? as u32
            );
        }
        hps_interface::Stage::Stage1 => {}
        hps_interface::Stage::Application => {
            println!(
                "FPGA boot count: {}",
                hps.read_register(registers::FPGA_BOOT_COUNT)?
            );
            println!(
                "FPGA loop count: {}",
                hps.read_register(registers::FPGA_LOOP_COUNT)?
            );
            print_model_score(hps, registers::USER_PRESENT_STATUS, "Presence status")?;
            print_model_score(hps, registers::USER_PRESENT_STATUS, "SPA status")?;
        }
        hps_interface::Stage::OneTimeInit => {
            println!(
                "Configured options: {:?}",
                OptionBytesConfigRequest::from_bits_truncate(
                    hps.read_register(registers::CONFIGURE_OPTION_BYTES)?
                )
            );
        }
    }
    Ok(())
}

fn print_model_score(hps: &mut dyn Hps, register: u8, description: &str) -> Result<()> {
    let raw_value = hps.read_register(register)?;
    print!("{}: 0x{:04x} (", description, raw_value);
    if raw_value & 0x8000 == 0 {
        print!("Image unusable");
    } else {
        print!("{}", (raw_value & 0xff) as i8);
    }
    print!(")");
    Ok(())
}
