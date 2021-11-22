// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::num::Wrapping;

#[derive(Default)]
pub struct ApplicationState {
    pub(crate) configuration: Configuration,
    pub(crate) status: Status,
    /// The number of times the FPGA has started. This is not part of `status`
    /// because `status` is only ever written by the FPGA, never read and we
    /// want a persistent count.
    pub(crate) fpga_boot_count: Wrapping<u16>,
    pub fpga_rom_version: u8,
}

/// The current configuration of the HPS. This is writable by the host over I2C
/// and readable by the FPGA soft CPU over SPI. The FPGA reads the entire struct
/// as bytes, hence the repr(C). The host writes parts individually via
/// registers.
#[repr(C)]
#[derive(Default)]
pub struct Configuration {
    pub enabled_features: u16,
}

/// Current status of the HPS. This is writable by the FPGA and is read by the
/// host.
#[repr(C)]
#[derive(Default)]
pub(crate) struct Status {
    pub(crate) score: i8,
    /// A copy of `Configuration::enabled_features` as used by the last
    /// inference loop.
    pub(crate) enabled_features: u16,
    pub(crate) loop_count: u16,
    pub(crate) usable: u8,
}

#[cfg(feature = "image-transfer")]
impl ApplicationState {
    pub fn i2c_image_transfer_enabled(&self) -> bool {
        self.configuration.enabled_features & 0x80 != 0
    }
}
