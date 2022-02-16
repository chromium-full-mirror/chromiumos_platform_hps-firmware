// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

// For documenation of what these commands do, see
// hps-mon/src/debug_commands.rs.

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum DebugCommand {
    Classification = 1,
    Transfer = 3,
    ExposureCalibration = 5,
    RotateImageClockwise = 16,
    SelfTest = 18,
    TestSpiFlashReads = 19,
    TestFpgaMcuComms = 20,
    TransferCount = 21,
}
