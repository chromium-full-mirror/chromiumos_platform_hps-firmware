// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the Nucleo STM32g071 board.

use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = hal::gpio::gpiob::PB0<Input<Floating>>;

pub struct Board {
    pub fpga_spi_cs: gpio::gpiob::PB0<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA5<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA6<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA7<Analog>,

    pub host_i2c_scl: gpio::gpiob::PB8<Analog>,
    pub host_i2c_sda: gpio::gpiob::PB9<Analog>,
}

impl Board {
    pub fn new(a: gpio::gpioa::Parts, b: gpio::gpiob::Parts) -> Self {
        Self {
            fpga_spi_cs: b.pb0,
            fpga_spi_clk: a.pa5,
            fpga_spi_cipo: a.pa6,
            fpga_spi_copi: a.pa7,

            host_i2c_scl: b.pb8,
            host_i2c_sda: b.pb9,
        }
    }
}
