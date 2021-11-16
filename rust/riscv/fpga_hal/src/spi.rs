// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::Infallible;
use embedded_hal::blocking::spi::Transfer;
use litex_pac::MCU_SPI;

pub struct McuSpi {
    mcu_spi: MCU_SPI,
}

impl McuSpi {
    pub fn new(mcu_spi: MCU_SPI) -> Self {
        Self { mcu_spi }
    }
}

impl Transfer<u8> for McuSpi {
    type Error = Infallible;

    fn transfer<'b>(&mut self, buffer: &'b mut [u8]) -> Result<&'b [u8], Self::Error> {
        // Wait until the MCU indicates that it's ready.
        while self.mcu_spi.flags.read().ready().bit_is_clear() {}

        // Pull CS low.
        self.mcu_spi.cr.modify(|_, w| w.cs().clear_bit());

        // Turn on SPI.
        self.mcu_spi.cr.modify(|_, w| w.en().set_bit());

        // Send and receive.
        for byte in buffer.iter_mut() {
            // Wait until TX register is empty.
            while self.mcu_spi.flags.read().txe().bit_is_clear() {}

            self.mcu_spi.txd.write(|w| unsafe { w.bits(*byte as u32) });

            // Wait until RX register is full.
            while self.mcu_spi.flags.read().rxe().bit_is_set() {}

            *byte = self.mcu_spi.rxd.read().bits() as u8;
        }

        // Turn off SPI.
        self.mcu_spi.cr.modify(|_, w| w.en().clear_bit());

        // Set CS high.
        self.mcu_spi.cr.modify(|_, w| w.cs().set_bit());

        Ok(buffer)
    }
}
