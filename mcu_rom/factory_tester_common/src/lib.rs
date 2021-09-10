// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Code shared between the test controller and the code that runs on the HPS
//! MCU.

#![no_std]

use core::convert::Infallible;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

/// Status code returned by the MCU to indicate that the test passed.
pub const TEST_STATUS_OK: u8 = 0;

/// Identifies a mode of failure when testing a HPS board. The numeric
/// identifiers are used both for communication between the test controller and
/// the MCU as well as for communicating the test status to a human - e.g. by
/// displaying a numeric error code on an LED display or by blinking and LED
/// some number of times.
#[derive(Debug, Copy, Clone, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Error {
    Internal = 1,
    SystemBootloader = 2,
    UnexpectedMcuId = 3,
    CameraI2c = 4,
    UnexpectedCameraId = 5,
    SpiFlashCommunication = 6,
    UnexpectedSpiFlashId = 7,
    FpgaNotStarted = 8,
    IncorrectImageData = 9,
    InterruptLine = 10,
    FwwpLine = 11,
    FirmwareAlreadyLoaded = 12,
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        Error::Internal
    }
}
