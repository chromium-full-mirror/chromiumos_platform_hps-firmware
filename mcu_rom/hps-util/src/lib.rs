// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod send_i2c;

use anyhow::Result;
use embedded_hal::blocking::i2c;
use ftd2xx_embedded_hal::libftd2xx;
use ftd2xx_embedded_hal::libftd2xx::Ft2232h;
use ftd2xx_embedded_hal::I2c;
use hps_interface::Hps;
use mcu_common::registers;

pub use send_i2c::SendI2c;

pub type Ft2232hHps = Hps<SendI2c<I2c<'static, Ft2232h>>, ftd2xx_embedded_hal::I2cError>;

pub fn print_device_list() -> Result<Vec<String>> {
    let mut v = Vec::<String>::new();
    for device in libftd2xx::list_devices()? {
        v.push(format!("{}: {}", device.serial_number, device.description));
    }
    Ok(v)
}

pub fn query_status<I, E>(hps: &mut Hps<I, E>) -> Result<String>
where
    I: i2c::WriteRead<Error = E> + i2c::Write<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
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
        Ok("No errors reported".to_string())
    } else {
        Ok(format!("Errors: {:?}", errors))
    }
}
