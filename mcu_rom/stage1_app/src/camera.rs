// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::boards::board;
use hal::prelude::*;
use log::error;
use log::info;

/// Camera HM01B0 I2C ID.
const CAMERA_I2C_ID: u8 = 0x24;

/// Camera HM01B0 registers.
const MODEL_ID_H: [u8; 2] = [0x00, 0x00];
const MODEL_ID_L: [u8; 2] = [0x00, 0x01];

// Returns camera's model ID (01B0).
pub fn read_camera_id(camera_i2c: &mut board::CameraI2c) {
    let mut camera_id: u16 = 0;
    let mut data = [0; 1];
    match camera_i2c.write_read(CAMERA_I2C_ID, &MODEL_ID_H, &mut data) {
        Ok(_) => {
            camera_id |= (data[0] as u16) << 8;
        }
        Err(_) => error!("Failed"),
    }
    match camera_i2c.write_read(CAMERA_I2C_ID, &MODEL_ID_L, &mut data) {
        Ok(_) => {
            camera_id |= data[0] as u16;
        }
        Err(_) => error!("Failed"),
    }
    info!("0x{:04x}", camera_id);
}

/// Provides a functionality to read one or more register addresses
/// from a camera. The register address is 2B size. The response is 1B size.
/// The request is a sequence of register addresses.
/// The response overwrites the first byte of each requested register address.
pub fn read_camera_reg(camera_i2c: &mut board::CameraI2c, addrs: &mut [u8]) {
    for addr_pair in addrs.chunks_exact_mut(2) {
        let mut data: [u8; 1] = [0];
        match camera_i2c.write_read(CAMERA_I2C_ID, addr_pair, &mut data) {
            Ok(_) => addr_pair[0] = data[0],
            Err(_) => error!("Failed to read 0x{:02x}{:02x}", addr_pair[0], addr_pair[1]),
        }
    }
}

/// Provides a functionality to write to one or more register addresses
/// to a camera. The register address is 2B size. The value is 1B size.
/// The request is a sequence of register addresses, followed by value.
pub fn write_camera_reg(camera_i2c: &mut board::CameraI2c, addrs_values: &mut [u8]) {
    for addr_pair_value in addrs_values.chunks_exact(3) {
        match camera_i2c.write(CAMERA_I2C_ID, addr_pair_value) {
            Ok(_) => {}
            Err(_) => error!(
                "Failed to write 0x{:02x}{:02x}[{:02x}]",
                addr_pair_value[0], addr_pair_value[1], addr_pair_value[2]
            ),
        }
    }
}
