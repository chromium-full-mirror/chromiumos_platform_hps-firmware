// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

// For documenation of what these commands do, see
// hps-mon/src/debug_commands.rs.

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum DebugCommand {
    Transfer = 3,
    SelfTest = 18,
    TestSpiFlashReads = 19,
    TestFpgaMcuComms = 20,
    TransferCount = 21,
    Histogram = 22,
    SetExposure = 23,
    SetMedianTarget = 24,
    DisableAutomaticExposure = 25,
    HardwareAe = 26,
}
