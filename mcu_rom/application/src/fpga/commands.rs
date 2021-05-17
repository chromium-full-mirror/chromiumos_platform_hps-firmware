// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Sent by the FPGA when it's awaiting a reply to a previous command or when
/// it's out-of-sync.
pub const POLL: u8 = 0;

/// MCU responds with the bytes of a configuration struct that is agreed upon by
/// both sides.
pub const READ_CONFIG_REGISTERS: u8 = 0x20;

/// Writes a struct agreed upon by both sides that containing status registers.
/// These status registers are then available to the host individually over i2c.
pub const WRITE_STATUS_REGISTERS: u8 = 0x21;

/// Resets the FPGA.
pub const RESET_FPGA: u8 = 0x40;

/// Sets the debug LED on or off based on whether the first byte of the payload
/// is 1 or 0 respectively.
pub const SET_DEBUG_LED: u8 = 0x60;

/// Logs the payload up to the first null character. Payload must be UTF-8.
pub const DEBUG_LOG: u8 = 0x61;

/// Payload contains bytes of image data to send to the host.
pub const DEBUG_IMAGE: u8 = 0x62;
