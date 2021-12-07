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

/// RO: Person detection status. See docs/host_device_i2c_protocol.md for
/// breakdown of individual bits.
pub const USER_PRESENT_STATUS: u8 = 8;

/// RO: Second person detection status. See docs/host_device_i2c_protocol.md for
/// breakdown of individual bits.
pub const SECOND_PERSON_STATUS: u8 = 9;

/// RO: Version register for hpsd.
pub const FIRMWARE_VERSION_HIGH: u8 = 10;
pub const FIRMWARE_VERSION_LOW: u8 = 11;

/// RO: Number of times the FPGA soft CPU has reported to us that it's starting.
pub const FPGA_BOOT_COUNT: u8 = 12;

/// RO: Number of times the FPGA soft CPU has run its main loop (since it last
/// booted).
pub const FPGA_LOOP_COUNT: u8 = 13;

/// RO: FPGA ROM version. This version has no semantic meaning. It is
/// incremented at the whim of developers and is mostly for debugging purposes.
pub const FPGA_ROM_VERSION: u8 = 14;

/// RO: SPI flash status registers as read at startup.
pub const SPI_FLASH_STATUS: u8 = 15;

/// RO: Debug tracing registers.
pub const DEBUG_IDX_REG: u8 = 16;
pub const DEBUG_VAL_REG: u8 = 17;

/// RO: Configuration bits for camera.
pub const CAMERA_CONFIG: u8 = 18;

#[cfg(feature = "dev")]
pub mod dev {
    /// RO: Reads will be sufficiently slow to cause an underrun.
    pub const SLOW_READ: u8 = 100;
}

/// RO: Whether there's a block of image data available to read.
#[cfg(feature = "image-transfer")]
pub const IMAGE_DATA_AVAILABLE: u8 = 101;

/// RO: Image data. Always 63 bytes.
#[cfg(feature = "image-transfer")]
pub const IMAGE_DATA: u8 = 102;
