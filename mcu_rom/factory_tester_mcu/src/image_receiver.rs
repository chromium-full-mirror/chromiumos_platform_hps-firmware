// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use hal::gpio;
use hal::gpio::Analog;
use log::error;
use log::info;

/// Receives image data from the FPGA over SPI and performs some basic checks of
/// that data.
pub(crate) fn validate_image_data(
    spi: &mut hal::stm32::SPI2,
    sck: &mut gpio::gpioa::PA0<Analog>,
    copi: &mut gpio::gpioa::PA4<Analog>,
) -> Result<(), Error> {
    initialize_spi(sck, copi, &spi);

    // Check that all pins (bits) can be observed with both a high and a low
    // value.
    let mut high_bits = 0;
    let mut low_bits = 0;
    for _ in 0..50000 {
        let byte = read_byte(spi)?;
        high_bits |= byte;
        low_bits |= !byte;
    }
    if high_bits != 0xff || low_bits != 0xff {
        error!(
            "Some bits not seen in some states. low=0x{:x}, high=0x{:x}",
            low_bits, high_bits
        );
        Err(Error::IncorrectImageData)
    } else {
        Ok(())
    }
}

fn initialize_spi(
    sck: &mut gpio::gpioa::PA0<Analog>,
    copi: &mut gpio::gpioa::PA4<Analog>,
    spi: &&mut stm32g0::stm32g071::SPI2,
) {
    hal::spi::PinSck::setup(sck);
    hal::spi::PinMosi::setup(copi);
    spi.cr1.write(|w| {
        w.rxonly().set_bit();
        // hps_tester.v sends data least-significant bit first.
        w.lsbfirst().set_bit();
        // hps_tester.v sends data CPHA=1 and CPOL=0.
        w.cpha().set_bit();
        // We receive data whenever there's a clock signal and don't use a chip
        // select line.
        w.ssm().set_bit()
    });
    spi.cr1.modify(|_, w| w.spe().set_bit());
}

fn read_byte(spi: &mut hal::stm32::SPI2) -> Result<u8, Error> {
    for _ in 0..5_000_000 {
        let status = spi.sr.read();
        if status.rxne().bit_is_set() {
            return Ok(spi.dr.read().bits() as u8);
        }
    }
    info!(
        "Timeout receiving image data from FPGA. spi status={:x}",
        spi.sr.read().bits()
    );
    Err(Error::IncorrectImageData)
}
