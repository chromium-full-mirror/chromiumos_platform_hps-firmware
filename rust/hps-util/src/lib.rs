// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod fake_hps;
mod send_i2c;

use anyhow::Result;
pub use fake_hps::FakeHps;
use hps_interface::Hps;
use mcu_common::registers;
pub use send_i2c::SendI2c;

pub fn print_status(hps: &mut dyn Hps) -> Result<()> {
    println!("Status register: {:?}", hps.status()?);
    match hps.check_errors() {
        Ok(_) => println!("No errors reported"),
        Err(error) => println!("{}", error),
    }
    println!(
        "FPGA boot count: {}",
        hps.read_register(registers::FPGA_BOOT_COUNT)?
    );
    println!(
        "FPGA loop count: {}",
        hps.read_register(registers::FPGA_LOOP_COUNT)?
    );
    Ok(())
}
