// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use bitflags::bitflags;

bitflags! {
    pub struct Features: u16 {
        const CLASSIFY_PRESENCE = 0x01;

        /// Allows the debug LED to be turned on and off by enabling / disabling
        /// this feature.
        const DEBUG_LED = 0x40;

        /// Whether to transfer images to the MCU for sending back to the host.
        /// This feature is for development and testing purposes only.
        const IMAGE_TRANSFER = 0x80;
    }
}
