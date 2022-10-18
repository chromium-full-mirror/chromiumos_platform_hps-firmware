// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

pub mod verification;

pub const HARDWARE_VERSION: u16 = 1;
pub const RO_VERSION: u16 = 5;
pub const COMBINED_VERSION: u16 = (HARDWARE_VERSION << 8) | RO_VERSION;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WriteProtectState {
    Asserted,
    Deasserted,
}
