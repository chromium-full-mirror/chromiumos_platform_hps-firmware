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
