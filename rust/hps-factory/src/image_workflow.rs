// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::hps_i2c::HpsI2c;
use crate::CaptureImagesConfig;
use crate::Config;
use crate::APPLICATION_BYTES;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use fpga_app::Features;
use host_dev_common::images::ImageType;
use host_dev_common::images::OutputDir;
use hps_interface::Hps;
use mcu_common::commands::Command;
use mcu_common::registers;

const IMAGE_DATA_PACKET_LENGTH: usize = 63;

pub(crate) fn capture_images<I, E>(
    i2c: &mut I,
    capture_images_config: &CaptureImagesConfig,
    config: &Config,
) -> Result<()>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut i2c = HpsI2c::new(i2c, config.clone());
    if !capture_images_config.skip_firmware_writes {
        i2c.write_stage1(APPLICATION_BYTES)?;
        i2c.execute_stage1().context("Execute application")?;
        if !capture_images_config.skip_bitstream_write {
            i2c.write_gateware().context("Write gateware")?;
        }
        i2c.write_soc_rom().context("Write SOC ROM")?;

        // Workaround for image capture not working on some devices if we've
        // written SPI flash since the last reset.
        i2c.open_hps()?.perform_command(Command::Reset)?;
    }
    i2c.execute_application().context("Execute application")?;

    let mut hps = i2c.open_hps()?;
    println!("HPS application started");

    // Configure enabled features.
    let mut enabled_features = Features::IMAGE_TRANSFER;
    hps.write_register(registers::ENABLED_FEATURES, enabled_features.bits())?;

    let mut buffer = Vec::new();
    let start = std::time::Instant::now();
    let mut output_dir = OutputDir::new(
        capture_images_config.out_dir.clone(),
        ImageType::Grayscale,
        Box::new(move |log_line| {
            println!("{:0.1}: {}", start.elapsed().as_secs_f32(), log_line);
            Ok(())
        }),
    )?;
    output_dir.set_raw_output(capture_images_config.raw);
    output_dir.set_latest_symlink(capture_images_config.latest_symlink);
    output_dir.set_wait_parent_dir_exists(capture_images_config.wait_parent_dir_exists);

    loop {
        if hps.read_register(registers::IMAGE_DATA_AVAILABLE)? == 1 {
            let packet =
                hps.read_register_bytes(registers::IMAGE_DATA, IMAGE_DATA_PACKET_LENGTH)?;
            buffer.extend_from_slice(&packet);
            output_dir.process_images_from_buffer(&mut buffer)?;
        } else if let Some(timeout) = capture_images_config.timeout_seconds {
            let time = output_dir.duration_without_images().as_secs();
            if time >= timeout {
                bail!("No images received for {} seconds", time);
            }
        }
    }
}
