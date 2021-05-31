// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Configuration for running on the HPS proto0 board.

use hal::gpio;
use hal::gpio::Analog;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::prelude::*;
use hal::serial;
use hal::stm32::I2C1;
use hal::stm32::USART2;
use i2c_peripheral::I2cError;
use i2c_peripheral::I2cPeripheral;

/// The type of the SPI CS pin from the FPGA once configured.
pub type FpgaSpiCsPin = gpio::gpioa::PA4<Input<Floating>>;

pub type HostBus = SerialHostBus;

pub type DebugLed = gpio::gpioa::PA1<Output<PushPull>>;
pub type FpgaProgramn = gpio::gpioc::PC15<Output<PushPull>>;
pub type SpiReady = gpio::gpioc::PC14<Output<PushPull>>;

pub struct Board {
    pub fpga_spi_cs: gpio::gpioa::PA4<Analog>,
    pub fpga_spi_clk: gpio::gpioa::PA5<Analog>,
    pub fpga_spi_cipo: gpio::gpioa::PA6<Analog>,
    pub fpga_spi_copi: gpio::gpioa::PA7<Analog>,

    /// Proto0 doesn't have I2C available for talking to the host, so we tunnel
    /// an equivalent protocol over UART.
    pub host_bus: SerialHostBus,

    pub debug_led: DebugLed,
    pub fpga_programn: FpgaProgramn,
    pub spi_ready: SpiReady,
}

impl Board {
    pub fn new(
        a: gpio::gpioa::Parts,
        _b: gpio::gpiob::Parts,
        c: gpio::gpioc::Parts,
        _i2c1: I2C1,
        usart2: USART2,
        rcc: &mut hal::rcc::Rcc,
    ) -> Self {
        let mut fpga_programn = c.pc15.into_push_pull_output();
        fpga_programn.set_high().unwrap();
        let mut spi_ready = c.pc14.into_push_pull_output();
        spi_ready.set_low().unwrap();
        Self {
            fpga_spi_cs: a.pa4,
            fpga_spi_clk: a.pa5,
            fpga_spi_cipo: a.pa6,
            fpga_spi_copi: a.pa7,

            host_bus: SerialHostBus::new(a.pa2, a.pa3, usart2, rcc),

            debug_led: a.pa1.into_push_pull_output(),
            fpga_programn,
            spi_ready,
        }
    }
}

enum BusState {
    Idle,
    Sending,
    Receiving,
}

/// An interface to UART that behaves somewhat like our I2C peripheral
/// interface. We expect to receive data in the following format:
/// * A byte == 0 indicates an I2C STOP condition.
/// * A byte with bit 7 set indicates a START write with bits 0-6 indicating the
///   length of the data to follow.
/// * A byte with bit 7 not set indicates a START read with bits 0-6 indicating
///   the length of the data to follow.
pub struct SerialHostBus {
    usart: hal::serial::usart::Serial<USART2, serial::BasicConfig>,
    state: BusState,
    remaining_bytes: u8,
}

impl SerialHostBus {
    fn new(
        tx: gpio::gpioa::PA2<Analog>,
        rx: gpio::gpioa::PA3<Analog>,
        usart2: USART2,
        rcc: &mut hal::rcc::Rcc,
    ) -> Self {
        let mut usart = usart2
            .usart(
                tx,
                rx,
                serial::BasicConfig::default().baudrate(115_200.bps()),
                rcc,
            )
            .map_err(drop)
            .unwrap();
        usart.listen(serial::Event::Rxne);
        Self {
            usart,
            state: BusState::Idle,
            remaining_bytes: 0,
        }
    }
}

impl I2cPeripheral for SerialHostBus {
    fn next_event(&mut self) -> i2c_peripheral::I2cEvent {
        use i2c_peripheral::I2cEvent;
        if !matches!(&self.state, BusState::Idle) {
            if self.remaining_bytes == 0 {
                self.usart.unlisten(serial::Event::Txe);
                self.state = BusState::Idle;
            }
        }
        if self.usart.is_pending(serial::Event::Rxne) {
            self.usart.unpend(serial::Event::Rxne);
            if let Ok(byte) = self.usart.read() {
                match &self.state {
                    BusState::Idle => {
                        self.remaining_bytes = byte & 0x7f;
                        if byte == 0 {
                            return I2cEvent::Stop;
                        }
                        if byte & 0x80 != 0 {
                            self.state = BusState::Receiving;
                            return I2cEvent::StartWrite;
                        }
                        self.state = BusState::Sending;
                        // Note, we only listen for the transmit buffer being
                        // empty when we're expecting to send data, otherwise
                        // we'll get repeated TXE interrupts, which would result
                        // in no lower priority interrupts getting a chance to
                        // run.
                        self.usart.listen(serial::Event::Txe);
                        return I2cEvent::StartRead;
                    }
                    BusState::Sending => {
                        return I2cEvent::Error(I2cError::BusError);
                    }
                    BusState::Receiving => {
                        self.remaining_bytes -= 1;
                        return I2cEvent::ByteReceived(byte);
                    }
                }
            }
        }
        if self.usart.is_pending(serial::Event::Txe) {
            self.usart.unpend(serial::Event::Txe);
            if matches!(&self.state, BusState::Sending) {
                return I2cEvent::NeedByte;
            }
        }
        I2cEvent::None
    }

    fn write_byte(&mut self, byte: u8) {
        self.remaining_bytes -= 1;
        let _ = self.usart.write(byte);
    }
}
