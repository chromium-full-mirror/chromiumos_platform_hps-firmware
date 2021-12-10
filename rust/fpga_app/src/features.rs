// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use bitflags::bitflags;

bitflags! {
    pub struct Features: u16 {
        const MODEL1 = 0x01;

        const MODEL2 = 0x02;

        /// Allows the debug LED to be turned on and off by enabling / disabling
        /// this feature.
        const DEBUG_LED = 0x40;

        /// Whether to transfer images to the MCU for sending back to the host.
        /// This feature is for development and testing purposes only.
        const IMAGE_TRANSFER = 0x80;

        /// Once enabled, the FPGA will commence testing reading the last 1MB of
        /// SPI flash repeatedly. The number of 1MB iterations will be reported
        /// in the loop counter and the number of bytes that were read
        /// incorrectly will be reported in the person_status register. Once
        /// started, this mode will run until reset. No other features will be
        /// processed and turning this feature off will have no effect.
        const SPI_FLASH_READ_TEST = 0x100;
    }
}
