// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub const MCU_RW: u8 = 0;

pub const FPGA_BITSTREAM: u8 = 1;

// We can write the whole SPI flash via bank 1 as well.
pub const SPI_FLASH: u8 = FPGA_BITSTREAM;

pub const SOC_ROM: u8 = 2;
