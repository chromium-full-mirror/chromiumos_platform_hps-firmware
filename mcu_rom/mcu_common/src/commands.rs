// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Resets the MCU.
pub const RESET: u16 = 1 << 0;

/// Launch the RW portion.
pub const LAUNCH1: u16 = 1 << 1;

/// Launch the application.
pub const LAUNCH_APP: u16 = 1 << 2;
