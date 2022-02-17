// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// The stage0 bootloader.
pub(crate) const STAGE0_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stage0.bin"));

/// A version of stage0 that is intended to be loaded directly from the system
/// bootloader and is located not at the start of flash so that the system
/// bootloader remains active.
pub(crate) const STAGE0_SYS_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/stage0.sys.bin"));

/// Non-release version of the MCU application. Supports features that should
/// never be in release builds such as image transfer and skipping the SPI hash
/// check.
pub(crate) const APPLICATION_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/application.bin"));

/// Release version of the MCU application.
pub(crate) const APPLICATION_RELEASE_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/application.release.bin"));

/// Initializes MCU option bytes including write protecting stage0. Also
/// supports removing temporary write protection from stage0 and erasing stage0.
pub(crate) const ONE_TIME_INIT_LOADER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init_loader.bin"));

/// Pre-install tester MCU program. This version is used when the system
/// bootloader is active and is loaded into RAM.
pub(crate) const PRE_INSTALL_TESTER_MCU_CODE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu.bin"));

/// Pre-install tester MCU program. This version is used when stage0 is present
/// and is written to MCU flash.
pub(crate) const PRE_INSTALL_TESTER_MCU_CODE_STAGE1: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu_stage1.bin"));

/// The FPGA bitstream.
pub(crate) const FPGA_BITSTREAM: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/hps_platform.bit"));

/// The FPGA ROM. Supports non-production features such as image transfer.
pub(crate) const FPGA_ROM_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/soc_rom.bin"));
