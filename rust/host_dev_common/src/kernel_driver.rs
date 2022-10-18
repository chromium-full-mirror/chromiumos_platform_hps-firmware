// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// Path to the sysfs node of the HPS kernel driver.
const HPS_I2C_SYSFS_PATH: &str = "/sys/bus/i2c/drivers/cros-hps";

// The i2c device id for HPS.
const HPS_I2C_DEVICE_ID: &str = "i2c-GOOG0020:00";

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PowerState {
    Active,
    Suspended,
    Unknown,
}

/// Unbinds the kernel driver, then rebinds it automatically when dropped.
#[non_exhaustive]
pub struct KernelDriverUnbinder {
    was_unbound: bool,
}

impl KernelDriverUnbinder {
    pub fn unbind() -> Result<Self> {
        let mut was_unbound = false;
        if is_bound() {
            unbind()?;
            was_unbound = true;
        }
        Ok(Self { was_unbound })
    }
}

impl Drop for KernelDriverUnbinder {
    fn drop(&mut self) {
        // Let the kernel take ownership of HPS again.
        if self.was_unbound {
            bind().unwrap();
        }
    }
}

fn device_path() -> PathBuf {
    Path::new(HPS_I2C_SYSFS_PATH).join(HPS_I2C_DEVICE_ID)
}

/// Returns whether the kernel driver is present and has bound the HPS.
fn is_bound() -> bool {
    device_path().exists()
}

pub fn unbind() -> Result<()> {
    fs::write(
        Path::new(HPS_I2C_SYSFS_PATH).join("unbind"),
        HPS_I2C_DEVICE_ID,
    )?;
    Ok(())
}

pub fn bind() -> Result<()> {
    fs::write(
        Path::new(HPS_I2C_SYSFS_PATH).join("bind"),
        HPS_I2C_DEVICE_ID,
    )?;
    Ok(())
}

pub fn power_state() -> PowerState {
    match fs::read_to_string(device_path().join("power/runtime_status")) {
        Ok(status) => match status.trim() {
            "suspended" => PowerState::Suspended,
            "active" => PowerState::Active,
            _ => PowerState::Unknown,
        },
        Err(_) => PowerState::Unknown,
    }
}
