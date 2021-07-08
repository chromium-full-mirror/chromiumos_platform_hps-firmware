// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the Nucleo STM32g071 board.

use hal::dmamux::DmaMuxIndex;
use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::i2c::Config;
use hal::prelude::*;
use hal::stm32::I2C1;
use hal::stm32::I2C2;
use hal::stm32::SPI1;
use hal::stm32::SPI2;
use hal::stm32::USART2;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = hal::gpio::gpiob::PB0<Input<Floating>>;
pub const FPGA_CS_EVENT: hal::exti::Event = hal::exti::Event::GPIO0;

pub type HostBus = Stm32g0I2cPeripheral;

// The LED on the Nucleo board is actually PA5, however that's already used for
// the SPI clock, so we arbitrarily use some other pin so that the code
// compiles.
pub type DebugLed = gpio::gpioa::PA1<Output<PushPull>>;

// The nucleo board doesn't have an FPGA hooked up to it, so we arbitrarily pick
// some pins.
pub type FpgaProgramn = gpio::gpioc::PC15<Output<PushPull>>;
pub type FpgaPowerGate = gpio::gpiob::PB4<Output<PushPull>>;
pub type SpiReady = gpio::gpioc::PC14<Output<PushPull>>;

pub type FpgaSpi = hal::stm32::SPI1;
pub type FlashSpi = hal::spi::Spi<
    hal::stm32::SPI2,
    (
        gpio::gpioa::PA0<Analog>, // SCK
        gpio::gpiob::PB6<Analog>, // CIPO
        gpio::gpiob::PB7<Analog>, // COPI
    ),
>;
pub type FlashSpiCs = gpio::gpiob::PB1<Output<PushPull>>;
pub type CameraI2c =
    hal::i2c::I2c<I2C2, gpio::gpiob::PB11<Output<OpenDrain>>, gpio::gpiob::PB10<Output<OpenDrain>>>;

/// GigaDevice
pub const SPI_FLASH_MANUFACTURER_ID: u8 = 0xC8;

/// GigaDevice GD25LQ128C/D
pub const SPI_FLASH_DEVICE_ID: [u8; 2] = [0x60, 0x18];

pub struct Board {
    pub fpga_spi_cs: gpio::gpiob::PB0<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA5<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA6<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA7<Analog>,

    pub host_bus: Stm32g0I2cPeripheral,

    pub debug_led: DebugLed,
    pub fpga_programn: FpgaProgramn,
    pub fpga_power_gate: FpgaPowerGate,
    pub spi_ready: SpiReady,
    pub fpga_spi: FpgaSpi,

    pub flash_spi_cs: Option<FlashSpiCs>,
    pub flash_spi: Option<FlashSpi>,
    pub camera_i2c: Option<CameraI2c>,
}

impl Board {
    pub fn new(
        a: gpio::gpioa::Parts,
        b: gpio::gpiob::Parts,
        c: gpio::gpioc::Parts,
        i2c1: I2C1,
        i2c2: I2C2,
        _usart2: USART2,
        spi1: SPI1,
        spi2: SPI2,
        rcc: &mut hal::rcc::Rcc,
        i2c_config: stm32g0_i2c_peripheral::Config,
    ) -> Self {
        let host_bus = Stm32g0I2cPeripheral::new(
            i2c1,
            rcc,
            b.pb9.into_open_drain_output(),
            b.pb8.into_open_drain_output(),
            mcu_common::HPS_ADDRESS,
            i2c_config,
        );

        // Nucleo board doesn't have SPI flash, so we pick some arbitrary pins.
        let flash_spi_cs = b.pb1.into_push_pull_output(); // J4-1
        let flash_spi_clk = a.pa0;
        let flash_spi_copi = b.pb7;
        let flash_spi_cipo = b.pb6;
        let flash_spi = spi2.spi(
            (flash_spi_clk, flash_spi_cipo, flash_spi_copi),
            hal::spi::MODE_0,
            1.mhz(),
            rcc,
        );
        let camera_i2c = i2c2.i2c(
            b.pb11.into_open_drain_output(), // SDA
            b.pb10.into_open_drain_output(), // SCL
            Config::new(400.khz()),
            rcc,
        );

        Self {
            fpga_spi_cs: b.pb0,
            fpga_spi_clk: a.pa5,
            fpga_spi_cipo: a.pa6,
            fpga_spi_copi: a.pa7,

            host_bus,

            debug_led: a.pa1.into_push_pull_output(),
            fpga_programn: c.pc15.into_push_pull_output(),
            fpga_power_gate: b.pb4.into_push_pull_output(),
            spi_ready: c.pc14.into_push_pull_output(),
            fpga_spi: spi1,

            flash_spi_cs: Some(flash_spi_cs),
            flash_spi: Some(flash_spi),
            camera_i2c: Some(camera_i2c),
        }
    }
}

pub const FPGA_SPI_MUX_RX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI1_RX;
pub const FPGA_SPI_MUX_TX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI1_TX;

pub unsafe fn fpga_spi_registers() -> *const hal::stm32::spi1::RegisterBlock {
    hal::stm32::SPI1::ptr()
}
