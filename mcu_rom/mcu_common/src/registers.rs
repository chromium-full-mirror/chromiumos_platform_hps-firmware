// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// RO: A magic value that can help to confirm that the device is a HPS.
pub const MAGIC: u8 = 0;

/// RO: Hardware + stage0 version.
pub const HARDWARE_VERSION: u8 = 1;

/// RO: System status.
pub const SYSTEM_STATUS: u8 = 2;

/// WO: Performs a system command.
pub const COMMAND: u8 = 3;

/// RO: Application version
pub const APPLICATION_VERSION: u8 = 4;

/// RO: Which memory banks can currently be written.
pub const MEMORY_BANK_AVAILABLE: u8 = 5;

/// RO: Errors. See individual error bits in the errors module.
pub const ERRORS: u8 = 6;

/// WO: Which application features are enabled.
pub const ENABLED_FEATURES: u8 = 7;

/// RO: If the user present feature is disabled implied by 0.
pub const USER_PRESENT_STATUS: u8 = 8;

#[cfg(feature = "dev")]
pub mod dev {
    /// RO: Reads will be sufficiently slow to cause an underrun.
    pub const SLOW_READ: u8 = 100;
}
