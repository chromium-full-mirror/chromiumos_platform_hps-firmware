// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use ftd2xx_embedded_hal::libftd2xx;
use ftd2xx_embedded_hal::libftd2xx::Ft2232h;
use ftd2xx_embedded_hal::I2c;
use hps_interface::Hps;
use mcu_common::registers;

pub type Ft2232hHps<'a> = Hps<I2c<'a, Ft2232h>, ftd2xx_embedded_hal::I2cError>;

pub fn print_device_list() -> Result<()> {
    for device in libftd2xx::list_devices()? {
        println!("{}: {}", device.serial_number, device.description);
    }
    Ok(())
}

pub fn query_status(hps: &mut Ft2232hHps) -> Result<()> {
    use std::convert::TryFrom;
    let error_bits = hps.read_register(registers::ERRORS)?;
    let errors: Vec<_> = (0..16)
        .filter_map(|bit| {
            let bit_value = 1 << bit;
            if error_bits & bit_value != 0 {
                mcu_common::Error::try_from(bit_value).ok()
            } else {
                None
            }
        })
        .collect();
    if errors.is_empty() {
        println!("No errors reported");
    } else {
        println!("Errors: {:?}", errors);
    }
    Ok(())
}
