// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use std::fs;
use std::path::Path;

/// Path to the sysfs node of the HPS kernel driver.
const HPS_I2C_SYSFS_PATH: &str = "/sys/bus/i2c/drivers/cros-hps";

// The i2c device id for HPS.
const HPS_I2C_DEVICE_ID: &str = "i2c-GOOG0020:00";

#[non_exhaustive]
pub struct KernelDriverUnbinder {
    was_unbound: bool,
}

impl KernelDriverUnbinder {
    pub fn unbind() -> Result<Self> {
        let mut was_unbound = false;
        let driver_path = Path::new(HPS_I2C_SYSFS_PATH);
        if driver_path.join(HPS_I2C_DEVICE_ID).exists() {
            fs::write(driver_path.join("unbind"), HPS_I2C_DEVICE_ID)?;
            was_unbound = true;
        }
        Ok(Self { was_unbound })
    }
}

impl Drop for KernelDriverUnbinder {
    fn drop(&mut self) {
        // Let the kernel take ownership of HPS again.
        if self.was_unbound {
            fs::write(
                Path::new(HPS_I2C_SYSFS_PATH).join("bind"),
                HPS_I2C_DEVICE_ID,
            )
            .unwrap();
        }
    }
}
