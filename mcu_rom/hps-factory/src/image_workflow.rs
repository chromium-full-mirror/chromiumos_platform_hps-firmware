// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::APPLICATION_BYTES;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use host_dev_common::images::ImageType;
use host_dev_common::images::OutputDir;
use hps_interface::Hps;
use mcu_common::registers;
use mcu_common::APPLICATION_START_ADDRESS;
use mcu_common::APPLICATION_VECTOR_ADDRESS;
use std::path::Path;

const IMAGE_DATA_PACKET_LENGTH: usize = 63;

pub(crate) fn capture_images<I, E>(i2c: &mut I, output_dir: &Path, config: Config) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config.clone());
    if config.skip_firmware_writes {
        if i2c.open_hps().is_err() {
            i2c.execute(APPLICATION_VECTOR_ADDRESS)?;
        }
    } else {
        i2c.write("application", APPLICATION_START_ADDRESS, APPLICATION_BYTES)?;
        i2c.execute(APPLICATION_VECTOR_ADDRESS)?;
        i2c.write_gateware()?;
        i2c.write_soc_rom()?;
    }

    let mut hps = i2c.open_hps()?;
    println!("HPS application started");

    // Enable image transfer.
    hps.write_register(registers::ENABLED_FEATURES, 0x80)?;

    let mut buffer = Vec::new();
    let start = std::time::Instant::now();
    let mut output_dir = OutputDir::new(
        output_dir.to_owned(),
        ImageType::Grayscale,
        Box::new(move |log_line| {
            println!("{:0.1}: {}", start.elapsed().as_secs_f32(), log_line);
            Ok(())
        }),
    )?;

    loop {
        if hps.read_register(registers::IMAGE_DATA_AVAILABLE)? == 1 {
            let packet =
                hps.read_register_bytes(registers::IMAGE_DATA, IMAGE_DATA_PACKET_LENGTH)?;
            buffer.extend_from_slice(&packet);
            output_dir.process_images_from_buffer(&mut buffer)?;
        }
    }
}
