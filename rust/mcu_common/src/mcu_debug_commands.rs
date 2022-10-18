// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::From;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum McuDebugCommand {
    ResetFpga = 1,
    ReadCameraId = 2,
    Ping = 3,
    ReadSpiFlash = 4,
    WriteSpiFlash = 5,
    EraseSpiFlash = 7,
    HashSpiFlash = 8,
    SpiFlashCrc = 9,
    SpiFlashReadSpeed = 10,
    SpiFlashWriteSpeed = 11,
    FpgaPowerOff = 12,
    FpgaPowerOn = 13,
    ReadCameraConfig = 14,
    SetCameraDigitalGain = 16,
    SetCameraAnalogGain = 17,
    SetCameraIntegration = 18,
    ReadCameraRegister = 19,
    SetCameraAeTarget = 20,
    TryStartFpga = 21,
    SetCameraBlcTarget = 22,
    SetBlockingMode = 23,
}
