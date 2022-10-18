// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Sent in response to a POLL command to indicate that we are ready to receive
/// a real command.
pub const READY: u8 = 1;

/// Value sent to the FPGA to indicate that the command was understood and the
/// response contains whatever payload is expected for that command.
pub const OK: u8 = 2;

/// Sent when we receive a command from the FPGA that we don't recognize.
pub const ERROR: u8 = 3;
