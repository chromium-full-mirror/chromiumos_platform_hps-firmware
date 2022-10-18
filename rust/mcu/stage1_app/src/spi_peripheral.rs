// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use hal::dma::Channel;
use hal::dma::Event::TransferError;
use hal::prelude::*;
use log::error;
use mcu_common::MemBlock;

/// The SPI interface with the FPGA. We act as the peripheral on the SPI bus,
/// with the FPGA as the controller.
pub struct SpiPeripheral {
    spi: board::FpgaSpi,
    tx_dma: hal::dma::C1,
    rx_dma: hal::dma::C2,
    mode: Mode,
    cs: board::FpgaSpiCsPin,
    cs_state: bool,
    /// Signal to inform the FPGA that we're ready for a SPI transfer. We set
    /// this high once we've configured DMA to send or receive via SPI. We set
    /// it low shortly after the transfer starts (CS line goes low).
    spi_ready: board::SpiReady,
}

/// The current mode of our SPI interface. We transition through these states in
/// order, skipping only `WaitingToSend` if there isn't a transfer in progress
/// when our response becomes ready.
enum Mode {
    /// We are ready to or are receiving data, which will be written into our
    /// buffer. Transmitted data is all 0s.
    Receive(MemBlock),
    /// Preparing our response. Ownership of the buffer has been transferred out
    /// to the code that will prepare the response. We ignore all received data
    /// and transmit all 0s.
    PreparingResponse,
    /// We have a response that is ready to send, but the controller is part way
    /// through a transmission. Continue to discard everything sent by the
    /// controller and send 0s. Once the transmission ends, then we prepare to
    /// send the response.
    WaitingToSend(MemBlock),
    /// We are ready to send or are sending a response. All received data will
    /// be discarded. After sending completes, we return back to receive mode.
    Send(MemBlock),
}

impl SpiPeripheral {
    pub(crate) fn new(
        spi: board::FpgaSpi,
        sck: impl hal::spi::PinSck<board::FpgaSpi>,
        miso: impl hal::spi::PinMiso<board::FpgaSpi>,
        mosi: impl hal::spi::PinMosi<board::FpgaSpi>,
        mut cs: board::FpgaSpiCsPin,
        tx_dma: hal::dma::C1,
        rx_dma: hal::dma::C2,
        spi_ready: board::SpiReady,
        buffer: MemBlock,
    ) -> SpiPeripheral {
        hal::spi::PinSck::setup(&sck);
        hal::spi::PinMiso::setup(&miso);
        hal::spi::PinMosi::setup(&mosi);
        cs.init_spi_chip_select();

        // Reset cr1.
        spi.cr1.write(|w| w);

        spi.cr2.write(|w| {
            // 8 bit data
            unsafe {
                w.ds().bits(0b111);
            }
            // Write each byte to memory as it is completed rather than
            // buffering 2 bytes in the FIFO. If we allow buffering of bytes,
            // then we can end up with the last byte of one packet being held
            // over until the start of the next packet.
            w.frxth().set_bit();
            // Enable DMA for receive. Note, TX is done later according to the
            // order specified in the reference manual.
            w.rxdmaen().set_bit()
        });

        // TODO: Enable interrupts for DMA errors.

        let mut spi_peripheral = Self {
            spi,
            tx_dma,
            rx_dma,
            mode: Mode::Receive(buffer),
            cs,
            cs_state: true,
            spi_ready,
        };
        spi_peripheral.configure_dma_channels();

        // Enable DMA for transmit.
        spi_peripheral.spi.cr2.modify(|_, w| w.txdmaen().set_bit());
        // Enable SPI.
        spi_peripheral.spi.cr1.modify(|_, w| w.spe().set_bit());

        spi_peripheral
    }

    pub(crate) fn send(&mut self, data: MemBlock) {
        // If the bus is not currently active (CS is high), then we configure
        // DMA straight away. Otherwise we'll configure DMA when CS becomes
        // high.
        if self.cs_state {
            self.mode = Mode::Send(data);
            self.configure_dma_channels();
        } else {
            self.mode = Mode::WaitingToSend(data);
        }
    }

    pub(crate) fn handle_rx_dma_interrupt(&mut self) {
        if self.rx_dma.event_occurred(TransferError) {
            // TODO: Report error properly.
            error!("DMA transfer error");
        }
    }

    pub(crate) fn handle_tx_dma_interrupt(&mut self) {
        if self.tx_dma.event_occurred(TransferError) {
            // TODO: Report error properly.
            error!("DMA transfer error");
        }
    }

    pub(crate) fn cs_changed(&mut self) -> Option<MemBlock> {
        self.cs_state = self.cs.is_high().unwrap_or_default();
        let mut result = None;
        if self.cs_state {
            // CS has gone high, indicating the end of a transfer.
            match core::mem::replace(&mut self.mode, Mode::PreparingResponse) {
                Mode::Receive(buffer) => result = Some(buffer),
                Mode::WaitingToSend(buffer) => self.mode = Mode::Send(buffer),
                Mode::Send(buffer) => self.mode = Mode::Receive(buffer),
                Mode::PreparingResponse => {}
            }
            // Discard any extra data left in the receive FIFO. If we were in
            // any mode except for `Mode::Receive` there will likely be data.
            while self.spi.sr.read().frlvl().bits() != 0 {
                self.spi.dr.read();
            }
            self.configure_dma_channels();
        } else {
            // Transfer has started, signal that we're not ready.
            self.spi_ready.set_low().unwrap();
        }
        result
    }

    fn configure_dma_channels(&mut self) {
        self.rx_dma.disable();
        self.tx_dma.disable();

        match &self.mode {
            Mode::Receive(buffer) => {
                // Fill the TX fifo with zero bytes. These bytes will then
                // repeat once the FIFO empties, resulting in us just sending
                // zeros while we receive and in subsequent states until we
                // reach the Send state.
                self.spi.dr.write(|w| w);
                self.spi.dr.write(|w| w);
                configure_dma_channel(
                    &mut self.rx_dma,
                    board::FPGA_SPI_MUX_RX,
                    hal::dma::Direction::FromPeripheral,
                    buffer,
                );
            }
            Mode::Send(buffer) => {
                configure_dma_channel(
                    &mut self.tx_dma,
                    board::FPGA_SPI_MUX_TX,
                    hal::dma::Direction::FromMemory,
                    buffer,
                );
            }
            _ => {
                // Don't signal that we're ready yet.
                return;
            }
        }
        // DMA has been configured. Signal that we're ready to receive SPI
        // communications.
        self.spi_ready.set_high().unwrap();
    }
}

fn configure_dma_channel(
    channel: &mut impl hal::dma::Channel,
    peripheral: hal::dmamux::DmaMuxIndex,
    direction: hal::dma::Direction,
    buffer: &MemBlock,
) {
    let spi_registers = unsafe { &*board::fpga_spi_registers() };
    let spi_dr_address = &spi_registers.dr as *const _ as u32;

    channel.set_direction(direction);
    channel.set_memory_address(buffer.as_ptr() as u32, true);
    channel.set_transfer_length(buffer.len() as u16);
    channel.set_peripheral_address(spi_dr_address, false);
    channel.set_word_size(hal::dma::WordSize::BITS8);
    channel.select_peripheral(peripheral);
    channel.enable();
}

// TODO: See if we can add support to the HAL crate for doing this. Currently
// the alternate function code is private and exposed via functions such as
// `PinSck::setup`.
pub(crate) trait InitSpiChipSelect {
    fn init_spi_chip_select(&mut self);
}

impl<T> InitSpiChipSelect for hal::gpio::gpioa::PA4<T> {
    fn init_spi_chip_select(&mut self) {
        unsafe {
            let gpioa = &*hal::stm32::GPIOA::ptr();
            gpioa.afrl.modify(|_, w| w.afsel4().bits(0));
            gpioa.moder.modify(|_, w| w.moder4().bits(2));
        }
    }
}

impl<T> InitSpiChipSelect for hal::gpio::gpiob::PB0<T> {
    fn init_spi_chip_select(&mut self) {
        unsafe {
            let gpiob = &*hal::stm32::GPIOB::ptr();
            gpiob.afrl.modify(|_, w| w.afsel0().bits(0));
            gpiob.moder.modify(|_, w| w.moder0().bits(2));
        }
    }
}

impl<T> InitSpiChipSelect for hal::gpio::gpioa::PA8<T> {
    fn init_spi_chip_select(&mut self) {
        unsafe {
            let gpioa = &*hal::stm32::GPIOA::ptr();
            gpioa.afrh.modify(|_, w| w.afsel8().bits(1));
            gpioa.moder.modify(|_, w| w.moder8().bits(2));
        }
    }
}
