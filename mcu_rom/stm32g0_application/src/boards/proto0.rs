// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the HPS proto0 board.

use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = gpio::gpioa::PA4<Input<Floating>>;

pub struct Board {
    pub fpga_spi_cs: gpio::gpioa::PA4<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA5<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA6<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA7<Analog>,

    // Proto0 doesn't have any host i2c connection. We arbitrarily use PB8 and
    // PB9, however these pins are not actually brought out anywhere accessible.
    pub host_i2c_sda: gpio::gpiob::PB9<Analog>,
    pub host_i2c_scl: gpio::gpiob::PB8<Analog>,
}

impl Board {
    pub fn new(a: gpio::gpioa::Parts, b: gpio::gpiob::Parts) -> Self {
        Self {
            fpga_spi_cs: a.pa4,
            fpga_spi_clk: a.pa5,
            fpga_spi_cipo: a.pa6,
            fpga_spi_copi: a.pa7,

            host_i2c_sda: b.pb9,
            host_i2c_scl: b.pb8,
        }
    }
}
