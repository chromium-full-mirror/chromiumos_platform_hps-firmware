// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
//

use hal::prelude::OutputPin;
use hal::prelude::StatefulOutputPin;
use hal::rcc::Rcc;
use mcu_common::Error;
use spi_memory::series25::Flash;

use crate::board;

// An array of segments describing the SPI Flash memory elements.
const SPI_FLASH_SEGMENTS: &[Segment] = &include!(concat!(env!("OUT_DIR"), "/spi_hash.inc"));

pub enum SegmentKind {
    Empty,
    #[allow(dead_code)] // Won't be constructed if no-hash-check feature is enabled.
    DataWithHash([u8; 32]),
}

pub struct Segment {
    pub length: usize,
    pub kind: SegmentKind,
}

pub struct FpgaDevice {
    /// Controls whether the FPGA is in reset. Set high to take the FPGA out of
    /// reset. We must be careful that we don't allow the FPGA to be taken out
    /// of reset without first checking the hash of the SPI flash. For that
    /// reason, this field is private to make it easier to audit usage.
    hold: board::FpgaProgramn,
    pub power_gate: board::FpgaPowerGate,
    pub(crate) spi_flash: Option<Flash<board::FlashSpiControl, board::FlashSpiCs>>,
}

impl FpgaDevice {
    pub(crate) fn new(
        fpga_hold: board::FpgaProgramn,
        power_gate: board::FpgaPowerGate,
        spi_flash: Option<Flash<board::FlashSpiControl, board::FlashSpiCs>>,
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

    #[cfg(feature = "dev")]
    pub(crate) fn reset_fpga(&mut self) {
        if self.hold.is_set_low().unwrap_or_default() {
            log::error!("Cannot reset FPGA since it isn't running");
            return;
        }
        log::info!("Resetting FPGA");
        // Hold programn low for long enough to reset the FPGA. Experimentally 2
        // iterations is sufficient, so we do 4 just to be sure.
        for _ in 0..4 {
            self.hold.set_low().unwrap();
        }
        self.hold.set_high().unwrap();
    }

    pub(crate) fn start_fpga(&mut self, rcc: &mut Rcc) -> Result<(), Error> {
        if cfg!(feature = "no-hash-check")
            || crate::spi_flash::hash(get_spi_flash(self)?, SPI_FLASH_SEGMENTS).is_ok()
        {
            // Once we start the FPGA, it will take over as the controller of
            // the SPI flash. We should not attept to communicate with the SPI
            // flash. Also, leaving the SPI interface to the SPI flash
            // initialized could interfere with the signals from the FPGA to the
            // SPI flash. So we uninitialize our interface.
            if let Some(spi_flash) = self.spi_flash.take() {
                crate::spi_flash::uninitialize_spi(spi_flash, rcc);
            }

            self.hold.set_high().unwrap();
            Ok(())
        } else {
            Err(Error::SpiFlashNotVerified)
        }
    }
}

pub(crate) fn get_spi_flash(
    fpga_device: &mut FpgaDevice,
) -> Result<&mut Flash<board::FlashSpiControl, board::FlashSpiCs>, Error> {
    if fpga_device.hold.is_set_high().unwrap() {
        // Requesting to do something with the SPI flash when the FPGA has
        // already been started is a bad request.
        Err(Error::HostI2cBadRequest)
    } else {
        fpga_device.spi_flash.as_mut().ok_or(Error::SpiFlash)
    }
}
