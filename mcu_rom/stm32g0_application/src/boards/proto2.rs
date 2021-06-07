// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the HPS proto2 board.

use hal::dmamux::DmaMuxIndex;
use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::prelude::*;
use hal::stm32::I2C1;
use hal::stm32::SPI1;
use hal::stm32::SPI2;
use hal::stm32::USART2;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = gpio::gpioa::PA8<Input<Floating>>;

pub type HostBus = Stm32g0I2cPeripheral;

pub type DebugLed = gpio::gpioa::PA1<Output<PushPull>>;
pub type FpgaProgramn = gpio::gpioc::PC15<Output<PushPull>>;
pub type SpiReady = gpio::gpioc::PC14<Output<PushPull>>;
pub type FpgaSpi = hal::stm32::SPI2;

/// GigaDevice
pub const SPI_FLASH_MANUFACTURER_ID: u8 = 0xC8;

/// GigaDevice GD25LQ128C/D
pub const SPI_FLASH_DEVICE_ID: [u8; 2] = [0x60, 0x18];

pub struct Board {
    pub fpga_spi_cs: gpio::gpioa::PA8<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA0<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA3<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA4<Analog>,

    pub host_bus: HostBus,

    pub debug_led: DebugLed,
    pub fpga_programn: FpgaProgramn,
    pub spi_ready: SpiReady,
    pub fpga_spi: FpgaSpi,

    pub flash_spi_cs: gpio::gpiob::PB0<Output<PushPull>>,
    pub flash_spi: hal::spi::Spi<
        hal::stm32::SPI1,
        (
            gpio::gpioa::PA5<Analog>,
            gpio::gpioa::PA6<Analog>,
            gpio::gpioa::PA7<Analog>,
        ),
    >,
}

impl Board {
    pub fn new(
        a: gpio::gpioa::Parts,
        b: gpio::gpiob::Parts,
        c: gpio::gpioc::Parts,
        i2c1: I2C1,
        _usart2: USART2,
        spi1: SPI1,
        spi2: SPI2,
        rcc: &mut hal::rcc::Rcc,
    ) -> Self {
        let mut fpga_programn = c.pc15.into_push_pull_output();
        fpga_programn.set_high().unwrap();
        let mut spi_ready = c.pc14.into_push_pull_output();
        spi_ready.set_low().unwrap();

        let host_bus = Stm32g0I2cPeripheral::new(
            i2c1,
            rcc,
            b.pb9.into_open_drain_output(),
            b.pb8.into_open_drain_output(),
            mcu_common::HPS_ADDRESS,
        );

        let flash_spi_cs = b.pb0.into_push_pull_output();
        let flash_spi_clk = a.pa5;
        let flash_spi_copi = a.pa7;
        let flash_spi_cipo = a.pa6;
        let flash_spi = spi1.spi(
            (flash_spi_clk, flash_spi_cipo, flash_spi_copi),
            hal::spi::MODE_0,
            1.mhz(),
            rcc,
        );

        Self {
            fpga_spi_cs: a.pa8,
            fpga_spi_clk: a.pa0,
            fpga_spi_cipo: a.pa3,
            fpga_spi_copi: a.pa4,

            host_bus,

            debug_led: a.pa1.into_push_pull_output(),
            fpga_programn,
            spi_ready,
            fpga_spi: spi2,

            flash_spi_cs,
            flash_spi,
        }
    }
}

pub const FPGA_SPI_MUX_RX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI2_RX;
pub const FPGA_SPI_MUX_TX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI2_TX;

pub unsafe fn fpga_spi_registers() -> *const hal::stm32::spi1::RegisterBlock {
    hal::stm32::SPI2::ptr()
}
