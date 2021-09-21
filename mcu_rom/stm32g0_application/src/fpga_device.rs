// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
//

use hal::prelude::OutputPin;
use rtt_target::rprintln;
use spi_memory::series25::Flash;

use crate::board;

pub struct FpgaDevice {
    pub hold: board::FpgaProgramn,
    pub power_gate: board::FpgaPowerGate,
    pub(crate) spi_flash: Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
}

impl FpgaDevice {
    pub(crate) fn new(
        fpga_hold: board::FpgaProgramn,
        power_gate: board::FpgaPowerGate,
        spi_flash: Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
    ) -> Self {
        FpgaDevice {
            hold: fpga_hold,
            power_gate: power_gate,
            spi_flash: spi_flash,
        }
    }
    pub(crate) fn reset_fpga(&mut self) {
        rprintln!("Resetting FPGA");
        // Hold programn low for long enough to reset the FPGA. Experimentally 2
        // iterations is sufficient, so we do 4 just to be sure.
        for _ in 0..4 {
            self.hold.set_low().unwrap();
        }
        self.hold.set_high().unwrap();
    }
}

pub(crate) fn get_spi_flash(
    fpga_device: &mut FpgaDevice,
) -> Option<&mut Flash<board::FlashSpi, board::FlashSpiCs>> {
    fpga_device.spi_flash.as_mut()
}
