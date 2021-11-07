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
}
