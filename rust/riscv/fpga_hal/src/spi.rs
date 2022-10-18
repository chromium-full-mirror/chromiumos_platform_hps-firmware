// Copyright 2021 The ChromiumOS Authors
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
        // Make sure SPI is disabled. It generally already will be, but just in
        // case we were called from a fault handler, it could be in an unknown
        // state.
        mcu_spi.cr.modify(|_, w| w.cs().clear_bit());
        Self { mcu_spi }
    }
}

impl Transfer<u8> for McuSpi {
    type Error = Infallible;

    fn transfer<'b>(&mut self, buffer: &'b mut [u8]) -> Result<&'b [u8], Self::Error> {
        // Wait until READY is high, which indicates that the MCU has configured
        // DMA and is ready to receive data.
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

        // Wait until READY goes low, which indicates that the MCU has observed
        // us pulling CS low. If it hasn't gone low yet, then the MCU might have
        // been too busy doing other stuff. We don't want the MCU to miss our CS
        // transitions, otherwise we can get data corruption due to mixing of
        // packets.
        while self.mcu_spi.flags.read().ready().bit_is_set() {}

        // Turn off SPI. Note this must be done after we wait for READY to go
        // low, since the gateware resets CS high when we disable SPI.
        self.mcu_spi.cr.modify(|_, w| w.en().clear_bit());

        // Set CS high.
        self.mcu_spi.cr.modify(|_, w| w.cs().set_bit());

        Ok(buffer)
    }
}
