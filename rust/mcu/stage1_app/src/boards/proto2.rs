// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the HPS proto2 board.

use crate::spi_dma_controller;
use hal::dmamux::DmaMuxIndex;
use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::gpio::Speed;
use hal::prelude::*;
use hal::stm32::I2C1;
use hal::stm32::I2C2;
use hal::stm32::SPI1;
use hal::stm32::SPI2;
use hal::stm32::TIM1;
use hal::stm32::TIM3;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = gpio::gpioa::PA8<Input<Floating>>;
pub const FPGA_CS_EVENT: hal::exti::Event = hal::exti::Event::GPIO8;

pub type HostBus = Stm32g0I2cPeripheral<I2C1>;

pub type InterruptPin = gpio::gpiob::PB8<Output<OpenDrain>>;
pub type DebugLed = gpio::gpioa::PA1<Output<PushPull>>;
pub type FpgaProgramn = gpio::gpioc::PC15<Output<PushPull>>;
pub type FpgaPowerGate = gpio::gpiob::PB5<Output<PushPull>>;
pub type SpiReady = gpio::gpioc::PC14<Output<PushPull>>;
pub type FpgaSpi = hal::stm32::SPI2;
pub type FlashSpi = hal::stm32::SPI1;
pub type FlashSpiControl = spi_dma_controller::SpiDmaController;
pub type FlashSpiCs = gpio::gpiob::PB0<Output<OpenDrain>>;
pub type CameraI2c =
    hal::i2c::I2c<I2C2, gpio::gpioa::PA12<Output<OpenDrain>>, gpio::gpioa::PA11<Output<OpenDrain>>>;

/// GigaDevice
pub const SPI_FLASH_MANUFACTURER_ID: u8 = 0xC8;

/// GigaDevice GD25LQ128C/D
pub const SPI_FLASH_DEVICE_ID: [u8; 2] = [0x60, 0x18];

pub struct Board {
    pub fpga_spi_cs: gpio::gpioa::PA8<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA0<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA3<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA4<Analog>,

    pub flash_spi_clk: gpio::gpioa::PA5<Analog>,
    pub flash_spi_cipo: gpio::gpioa::PA6<Analog>,
    pub flash_spi_copi: gpio::gpioa::PA7<Analog>,

    pub host_bus: HostBus,

    pub interrupt_pin: InterruptPin,
    pub debug_led: DebugLed,
    pub fpga_programn: FpgaProgramn,
    pub fpga_power_gate: FpgaPowerGate,
    pub spi_ready: SpiReady,
    pub fpga_spi: FpgaSpi,

    pub flash_spi: FlashSpi,
    pub flash_spi_cs: FlashSpiCs,
    pub camera_i2c: CameraI2c,
    pub camera_mclk: gpio::gpiob::PB1<Analog>,
    // TODO: Change to TIM1 once stm32g0xx-hal is updated beyond 2021-03-18. PB1
    // can use TIM1, but only as an inverted pin. Support for inverted pins was
    // added on that date. Using TIM1 should allow us to increase the clock
    // speed back to 64MHz.
    pub camera_mclk_tim: TIM3,
}

impl Board {
    pub fn new(
        a: gpio::gpioa::Parts,
        b: gpio::gpiob::Parts,
        c: gpio::gpioc::Parts,
        i2c1: I2C1,
        i2c2: I2C2,
        spi1: SPI1,
        spi2: SPI2,
        _tim1: TIM1,
        tim3: TIM3,
        rcc: &mut hal::rcc::Rcc,
        i2c_config: stm32g0_i2c_peripheral::Config,
    ) -> Self {
        let mut fpga_programn = c.pc15.into_push_pull_output();
        fpga_programn.set_high().unwrap();
        let fpga_power_gate = b.pb5.into_push_pull_output();
        let mut spi_ready = c.pc14.into_push_pull_output();
        spi_ready.set_low().unwrap();

        let sda = b.pb7.into_open_drain_output_in_state(PinState::High);
        let scl = b.pb6.into_open_drain_output_in_state(PinState::High);

        let host_bus = Stm32g0I2cPeripheral::new(i2c1, rcc, sda, scl, i2c_config);

        let flash_spi_cs = b.pb0.into_open_drain_output_in_state(PinState::High);
        let flash_spi_clk = a.pa5.set_speed(Speed::VeryHigh);
        let flash_spi_copi = a.pa7.set_speed(Speed::VeryHigh);
        let flash_spi_cipo = a.pa6.set_speed(Speed::VeryHigh);

        let sda = a.pa12.into_open_drain_output_in_state(PinState::High);
        let scl = a.pa11.into_open_drain_output_in_state(PinState::High);

        let camera_i2c = i2c2.i2c(sda, scl, hal::i2c::Config::new(120.khz()), rcc);

        Self {
            fpga_spi_cs: a.pa8,
            fpga_spi_clk: a.pa0,
            fpga_spi_cipo: a.pa3.set_speed(Speed::High),
            fpga_spi_copi: a.pa4,

            flash_spi_clk,
            flash_spi_cipo,
            flash_spi_copi,

            host_bus,

            interrupt_pin: b.pb8.into_open_drain_output_in_state(PinState::High),
            debug_led: a.pa1.into_push_pull_output(),
            fpga_programn,
            fpga_power_gate,
            spi_ready,
            fpga_spi: spi2,

            flash_spi: spi1,
            flash_spi_cs,
            camera_i2c,
            camera_mclk: b.pb1,
            camera_mclk_tim: tim3,
        }
    }
}

pub const FLASH_SPI_MUX_RX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI1_RX;
pub const FLASH_SPI_MUX_TX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI1_TX;

pub unsafe fn flash_spi_registers() -> *const hal::stm32::spi1::RegisterBlock {
    hal::stm32::SPI1::ptr()
}

pub const FPGA_SPI_MUX_RX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI2_RX;
pub const FPGA_SPI_MUX_TX: DmaMuxIndex = hal::dmamux::DmaMuxIndex::SPI2_TX;

pub unsafe fn fpga_spi_registers() -> *const hal::stm32::spi1::RegisterBlock {
    hal::stm32::SPI2::ptr()
}
