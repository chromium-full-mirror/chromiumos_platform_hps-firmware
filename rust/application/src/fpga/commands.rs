// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::From;
use num_enum::IntoPrimitive;
use num_enum::TryFromPrimitive;

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum Action {
    /// Sent by the FPGA when it's awaiting a reply to a previous command or
    /// when it's out-of-sync.
    Poll = 0,

    /// MCU responds with values in camera's register.
    ReadCameraI2c = 0x10,

    /// MCU writes requested values to camera's register.
    WriteCameraI2c = 0x11,

    /// MCU responds with the bytes of a configuration struct that is agreed
    /// upon by both sides.
    ReadConfigRegisters = 0x20,

    /// Writes a struct agreed upon by both sides that containing status
    /// registers. These status registers are then available to the host
    /// individually over i2c.
    WriteStatusRegisters = 0x21,

    /// Report that the FPGA has booted. We'll update the register that reports
    /// how many times the FPGA has booted. Takes a 1 byte argument that is a
    /// SOC ROM version number.
    ReportFpgaBoot = 0x22,

    /// Reports an error.
    ReportFpgaError = 0x23,

    /// Reports a panic.
    ReportFpgaPanic = 0x24,

    /// Trigger a single frame from the camera in N microseconds, where N is
    /// encoded as a little-endian u32.
    TriggerFrame = 0x25,

    /// Sets the debug LED on or off based on whether the first byte of the
    /// payload is 1 or 0 respectively.
    SetDebugLed = 0x60,

    /// Logs the payload up to the first null character. Payload must be UTF-8.
    #[cfg(feature = "dev")]
    DebugLog = 0x61,

    /// Payload contains bytes of image data to send to the host.
    #[cfg(feature = "image-transfer")]
    DebugImage = 0x62,

    /// Returns the next debug command sent over RTT if any.
    #[cfg(feature = "dev")]
    GetDebugCommand = 0x63,

    /// MCU will respond with the same data that was sent by the FPGA. This is
    /// used for testing the reliability of FPGA<->MCU communication. Note, the
    /// first byte sent by the FPGA actually gets overwritten by the command.
    Echo = 0x64,
}
