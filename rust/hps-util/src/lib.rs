// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod fake_hps;
pub mod monitor;
mod send_i2c;

use anyhow::Result;
pub use fake_hps::FakeHps;
use fpga_app::Features;
use hps_interface::Hps;
use mcu_common::registers::Register;
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
                hps.read_register(Register::HardwareVersion)?
            );
        }
        hps_interface::Stage::Stage1 => {
            let crash_report = hps.crash_report()?;
            if !crash_report.is_empty() {
                println!("Previous crash: {}", crash_report);
            }
            println!(
                "Stage1 version: 0x{:x}",
                ((hps.read_register(Register::FirmwareVersionHigh)? as u32) << 16)
                    | hps.read_register(Register::FirmwareVersionLow)? as u32
            );
            println!("{}", hps.read_part_ids()?);
        }
        hps_interface::Stage::Application => {
            let crash_report = hps.fpga_crash_report()?;
            if !crash_report.is_empty() {
                println!("FPGA crash: {}", crash_report);
            }
            println!(
                "FPGA boot count: {}",
                hps.read_register(Register::FpgaBootCount)?
            );
            println!(
                "FPGA loop count: {}",
                hps.read_register(Register::FpgaLoopCount)?
            );
            println!(
                "Enabled features: {:?}",
                Features::from_bits_truncate(hps.read_register(Register::EnabledFeatures)?)
            );
            print_model_score(hps, Register::UserPresentStatus, "Presence status")?;
            print_model_score(hps, Register::SecondPersonStatus, "SPA status")?;
            println!("Debug1: 0x{:04x}", hps.read_register(Register::Debug1)?);
            println!("Debug2: 0x{:04x}", hps.read_register(Register::Debug2)?);
            println!("Debug3: 0x{:04x}", hps.read_register(Register::Debug3)?);
        }
        hps_interface::Stage::OneTimeInit => {
            println!(
                "Configured options: {:?}",
                OptionBytesConfigRequest::from_bits_truncate(
                    hps.read_register(Register::ConfigurationOptionBytes)?
                )
            );
        }
    }
    Ok(())
}

fn print_model_score(hps: &mut dyn Hps, register: Register, description: &str) -> Result<()> {
    let raw_value = hps.read_register(register)?;
    print!("{}: 0x{:04x} (", description, raw_value);
    if raw_value & 0x8000 == 0 {
        print!("Image unusable");
    } else {
        print!("{}", (raw_value & 0xff) as i8);
    }
    println!(")");
    Ok(())
}
