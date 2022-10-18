// Copyright 2021 The ChromiumOS Authors
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

    /// Erase the SPI flash in preparation for writing new FPGA bitstream
    /// and application into it.
    EraseSpiFlash = 1 << 4,

    /// Signal an interrupt to the MLB. This is used for testing that the
    /// interrupt pin works.
    #[cfg(feature = "dev")]
    MlbInterrupt = 1 << 5,

    /// Trigger a deliberate panic on the MCU. This is used for testing panic
    /// handling.
    TriggerMcuPanic = 1 << 13,

    /// Writes test data to SPI flash. Only valid before LaunchApp is invoked,
    /// since after that the MCU does not have access to the SPI flash.
    WriteSpiFlashTestData = 1 << 14,

    /// Erase at least the first page of stage0 and reset into the system
    /// bootloader. Unlocks write protection, if necessary. Will fail if write
    /// protection is permanent (RDP level 2).
    EraseStage0Start = 1 << 15,
}
