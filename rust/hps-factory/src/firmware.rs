// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::borrow::Cow;
use std::path::PathBuf;

/// The stage0 bootloader.
pub(crate) const STAGE0_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stage0.bin"));

/// Initializes MCU option bytes including write protecting stage0. Also
/// supports removing temporary write protection from stage0 and erasing stage0.
pub(crate) const ONE_TIME_INIT_LOADER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init_loader.bin"));

/// As above, but for use with version 0x0101 of stage0, which loads stage1 at
/// address 0x08010000.
pub(crate) const ONE_TIME_INIT_LOADER_LEGACY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init_loader_legacy.bin"));

/// Pre-install tester MCU program. This version is used when the system
/// bootloader is active and is loaded into RAM.
pub(crate) const PRE_INSTALL_TESTER_MCU_CODE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu.bin"));

/// Pre-install tester MCU program. This version is used when stage0 is present
/// and is written to MCU flash.
pub(crate) const PRE_INSTALL_TESTER_MCU_CODE_STAGE1: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu_stage1.bin"));

impl crate::Config {
    pub(crate) fn fpga_bitstream(&self) -> Result<Cow<'static, [u8]>> {
        read_file_or_default(
            "--fpga-bitstream",
            &self.fpga_bitstream,
            include_bytes!(concat!(env!("OUT_DIR"), "/hps_platform.bit")),
        )
    }

    pub(crate) fn fpga_rom(&self) -> Result<Cow<'static, [u8]>> {
        read_file_or_default(
            "--fpga-rom",
            &self.fpga_rom,
            include_bytes!(concat!(env!("OUT_DIR"), "/soc_rom.bin")),
        )
    }

    pub(crate) fn mcu_rom(&self) -> Result<Cow<'static, [u8]>> {
        read_file_or_default(
            "--mcu-rom",
            &self.mcu_rom,
            include_bytes!(concat!(env!("OUT_DIR"), "/application.bin")),
        )
    }
}

/// Returns the contents of `path` if specified, or `default` if no path is
/// supplied. If `default` is empty, then an error is reported that `flag` must
/// be supplied.
fn read_file_or_default(
    flag: &str,
    opt_path: &Option<PathBuf>,
    default: &'static [u8],
) -> Result<Cow<'static, [u8]>> {
    if let Some(path) = opt_path {
        return Ok(Cow::Owned(std::fs::read(path).with_context(|| {
            format!(
                "Failed to open '{}' that was specified by the flag {}",
                path.display(),
                flag
            )
        })?));
    }
    if default.is_empty() {
        bail!(
            "{} must be supplied since it is not built-in to the binary.",
            flag
        );
    }
    Ok(Cow::Borrowed(default))
}
