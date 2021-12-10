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
    ClassificationOff = 1,
    ClassificationOn = 2,
    TransferOff = 3,
    TransferOn = 4,
    ExposureCalibration = 5,
    RotateImageClockwiseOff = 16,
    RotateImageClockwiseOn = 17,
    SelfTest = 18,
    TestSpiFlashReads = 19,
}
