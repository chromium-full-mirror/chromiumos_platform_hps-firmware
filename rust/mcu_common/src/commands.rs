// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::From;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum Command {
    /// Resets the MCU.
    Reset = 1 << 0,

    /// Launch the RW portion.
    Launch1 = 1 << 1,

    /// Launch the application.
    LaunchApp = 1 << 2,

    /// Erase the RW region of flash in preparation for writing a new
    /// stage1 image into it.
    EraseStage1 = 1 << 3,

    /// Writes test data to SPI flash. Only valid before LaunchApp is invoked,
    /// since after that the MCU does not have access to the SPI flash.
    WriteSpiFlashTestData = 1 << 14,

    /// Erase the first page of stage0 so that the system bootloader is
    /// activated on the next reboot. This command will fail if stage0 has been
    /// locked as read-only. After this command completes, the MCU will be
    /// reset, although that reset will likely result in a hard fault.
    EraseStage0Start = 1 << 15,
}
