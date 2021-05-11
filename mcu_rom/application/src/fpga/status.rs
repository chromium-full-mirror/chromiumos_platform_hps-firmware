// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Value sent to the FPGA to indicate that the command was understood and the
/// response contains whatever payload is expected for that command.
pub const OK: u8 = 2;
