// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#[derive(Default)]
pub struct ApplicationState {
    // TODO: Implement reading of configuration over SPI.
    #[allow(dead_code)]
    pub(crate) configuration: Configuration,
}

/// The current configuration of the HPS. This is writable by the host over I2C
/// and readable by the FPGA soft CPU over SPI. The FPGA reads the entire struct
/// as bytes, hence the repr(C). The host writes parts individually via
/// registers.
#[repr(C)]
#[derive(Default)]
pub(crate) struct Configuration {
    pub(crate) enabled_features: u16,
}
