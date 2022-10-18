// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::borrow::Cow;
use std::path::PathBuf;

/// Initializes MCU option bytes including write protecting stage0. Also
/// supports removing temporary write protection from stage0 and erasing stage0.
pub(crate) const ONE_TIME_INIT_LOADER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init_loader.bin"));

/// As above, but for use with version 0x0101 of stage0, which loads stage1 at
/// address 0x08010000.
pub(crate) const ONE_TIME_INIT_LOADER_LEGACY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/one_time_init_loader_legacy.bin"));

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

    pub(crate) fn stage0_rom(&self) -> Result<Cow<'static, [u8]>> {
        read_file_or_default(
            "--stage0-rom",
            &self.stage0_rom,
            include_bytes!(concat!(env!("OUT_DIR"), "/stage0.bin")),
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

impl crate::Config {
    pub(crate) fn write_firmware_files(
        &self,
        write_config: &crate::WriteEmbeddedFirmwareConfig,
    ) -> Result<()> {
        let out = &write_config.out_dir;
        std::fs::create_dir_all(out)?;
        if let Ok(bytes) = self.stage0_rom() {
            std::fs::write(out.join("stage0.bin"), bytes)?;
        }
        if let Ok(bytes) = self.mcu_rom() {
            std::fs::write(out.join("mcu_rom.bin"), bytes)?;
        }
        if let Ok(bytes) = self.fpga_rom() {
            std::fs::write(out.join("fpga_rom.bin"), bytes)?;
        }
        if let Ok(bytes) = self.fpga_bitstream() {
            std::fs::write(out.join("fpga_bitstream.bin"), bytes)?;
        }
        std::fs::write(
            out.join("one_time_init_loader.bin"),
            ONE_TIME_INIT_LOADER_BYTES,
        )?;
        std::fs::write(
            out.join("one_time_init_loader_legacy.bin"),
            ONE_TIME_INIT_LOADER_LEGACY_BYTES,
        )?;
        Ok(())
    }
}
