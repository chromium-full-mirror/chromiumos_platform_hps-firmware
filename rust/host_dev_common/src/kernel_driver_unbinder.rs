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
pub struct KernelDriverUnbinder {}

impl KernelDriverUnbinder {
    pub fn unbind() -> Result<Self> {
        bind_kernel_driver(false)?;
        Ok(Self {})
    }
}

impl Drop for KernelDriverUnbinder {
    fn drop(&mut self) {
        // Let the kernel take ownership of HPS again.
        bind_kernel_driver(true).unwrap();
    }
}

fn bind_kernel_driver(bind: bool) -> Result<()> {
    let driver_path = Path::new(HPS_I2C_SYSFS_PATH);
    if !driver_path.exists() {
        return Ok(());
    }
    let op = if bind { "bind" } else { "unbind" };
    fs::write(driver_path.join(op), HPS_I2C_DEVICE_ID)?;
    Ok(())
}
