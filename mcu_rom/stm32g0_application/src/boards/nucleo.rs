// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the Nucleo STM32g071 board.

use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::stm32::I2C1;
use hal::stm32::USART2;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = hal::gpio::gpiob::PB0<Input<Floating>>;

pub type HostBus = Stm32g0I2cPeripheral;

// The LED on the Nucleo board is actually PA5, however that's already used for
// the SPI clock, so we arbitrarily use some other pin so that the code
// compiles.
pub type DebugLed = gpio::gpioa::PA1<Output<PushPull>>;

// The nucleo board doesn't have an FPGA hooked up to it, so we arbitrarily pick
// some pin.
pub type FpgaProgramn = gpio::gpioc::PC15<Output<PushPull>>;

pub struct Board {
    pub fpga_spi_cs: gpio::gpiob::PB0<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA5<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA6<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA7<Analog>,

    pub host_bus: Stm32g0I2cPeripheral,

    pub debug_led: DebugLed,
    pub fpga_programn: FpgaProgramn,
}

impl Board {
    pub fn new(
        a: gpio::gpioa::Parts,
        b: gpio::gpiob::Parts,
        c: gpio::gpioc::Parts,
        i2c1: I2C1,
        _usart2: USART2,
        rcc: &mut hal::rcc::Rcc,
    ) -> Self {
        let host_bus = Stm32g0I2cPeripheral::new(
            i2c1,
            rcc,
            b.pb9.into_open_drain_output(),
            b.pb8.into_open_drain_output(),
            mcu_common::HPS_ADDRESS,
        );
        Self {
            fpga_spi_cs: b.pb0,
            fpga_spi_clk: a.pa5,
            fpga_spi_cipo: a.pa6,
            fpga_spi_copi: a.pa7,

            host_bus,

            debug_led: a.pa1.into_push_pull_output(),
            fpga_programn: c.pc15.into_push_pull_output(),
        }
    }
}
