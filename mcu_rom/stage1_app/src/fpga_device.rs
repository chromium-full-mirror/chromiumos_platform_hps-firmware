// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
//

use hal::prelude::{OutputPin, StatefulOutputPin};
use log::info;
use mcu_common::Error;
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
        let mut device = FpgaDevice {
            hold: fpga_hold,
            power_gate: power_gate,
            spi_flash: spi_flash,
        };
        // Hold FPGA in reset until it is in the verified state.
        device.hold.set_low().unwrap();

        device
    }
    pub(crate) fn reset_fpga(&mut self) {
        info!("Resetting FPGA");
        // Hold programn low for long enough to reset the FPGA. Experimentally 2
        // iterations is sufficient, so we do 4 just to be sure.
        for _ in 0..4 {
            self.hold.set_low().unwrap();
        }
        self.hold.set_high().unwrap();
    }

    pub(crate) fn start_fpga(&mut self) {
        // TODO: If the SPI flash hasn't been validated yet, this should report
        // an error and refuse to take the FPGA out of reset.
        self.hold.set_high().unwrap();
    }
}

pub(crate) fn get_spi_flash(
    fpga_device: &mut FpgaDevice,
) -> Result<&mut Flash<board::FlashSpi, board::FlashSpiCs>, Error> {
    if fpga_device.hold.is_set_high().unwrap() {
        // Requesting to do something with the SPI flash when the FPGA has
        // already been started is a bad request.
        Err(Error::HostI2cBadRequest)
    } else {
        fpga_device.spi_flash.as_mut().ok_or(Error::SpiFlash)
    }
}
