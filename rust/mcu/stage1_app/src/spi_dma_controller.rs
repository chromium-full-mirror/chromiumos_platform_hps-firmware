// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::board;
use hal::dma::Channel;

pub struct SpiDmaController {
    spi: hal::stm32::SPI1,
    tx_dma: hal::dma::C3,
    rx_dma: hal::dma::C4,
}

#[derive(Debug)]
pub enum SpiError {}

impl SpiDmaController {
    pub(crate) fn new(
        spi: hal::stm32::SPI1,
        sck: impl hal::spi::PinSck<hal::stm32::SPI1>,
        miso: impl hal::spi::PinMiso<hal::stm32::SPI1>,
        mosi: impl hal::spi::PinMosi<hal::stm32::SPI1>,
        tx_dma: hal::dma::C3,
        rx_dma: hal::dma::C4,
    ) -> SpiDmaController {
        hal::spi::PinSck::setup(&sck);
        hal::spi::PinMiso::setup(&miso);
        hal::spi::PinMosi::setup(&mosi);

        let dma_controller = Self {
            spi: spi,
            tx_dma: tx_dma,
            rx_dma: rx_dma,
        };
        dma_controller.spi.cr1.write(|w| {
            w.cpha().clear_bit();
            w.cpol().clear_bit();
            w.mstr().set_bit();
            // Set baud rate to PCLK/2
            unsafe {
                w.br().bits(0b000);
            }
            w.lsbfirst().clear_bit();
            w.ssm().set_bit();
            w.ssi().set_bit();
            w.rxonly().clear_bit();
            w.bidimode().clear_bit();
            w.dff().clear_bit()
        });
        dma_controller.spi.cr2.write(|w| {
            // Set data size to 8 bit
            unsafe {
                w.ds().bits(0b111);
            }
            w.rxdmaen().set_bit();
            w.ssoe().clear_bit();
            w.frxth().set_bit()
        });
        // Enable DMA for transmit after enabling receiving.
        dma_controller.spi.cr2.modify(|_, w| w.txdmaen().set_bit());
        // Enable SPI.
        dma_controller.spi.cr1.modify(|_, w| w.spe().set_bit());

        dma_controller
    }
}

fn configure_dma_channel(
    channel: &mut impl hal::dma::Channel,
    peripheral: hal::dmamux::DmaMuxIndex,
    direction: hal::dma::Direction,
    buf: &mut [u8],
) {
    let spi_registers = unsafe { &*board::flash_spi_registers() };
    let spi_dr_address = &spi_registers.dr as *const _ as u32;

    channel.disable(); // Configuration must be applied on disabled DMA
    channel.select_peripheral(peripheral);
    channel.set_peripheral_address(spi_dr_address, false);
    channel.set_memory_address(buf.as_ptr() as u32, true);
    channel.set_direction(direction);
    channel.set_transfer_length(buf.len() as u16);
    channel.set_priority_level(hal::dma::Priority::VeryHigh);
    channel.set_word_size(hal::dma::WordSize::BITS8);
    channel.set_circular_mode(false);
}

impl embedded_hal::blocking::spi::Transfer<u8> for SpiDmaController {
    type Error = SpiError;

    fn transfer<'w>(&mut self, buf: &'w mut [u8]) -> Result<&'w [u8], SpiError> {
        // Configure and enable RX channel before the TX channel.
        configure_dma_channel(
            &mut self.rx_dma,
            board::FLASH_SPI_MUX_RX,
            hal::dma::Direction::FromPeripheral,
            buf,
        );
        self.rx_dma.enable();

        configure_dma_channel(
            &mut self.tx_dma,
            board::FLASH_SPI_MUX_TX,
            hal::dma::Direction::FromMemory,
            buf,
        );
        self.tx_dma.listen(hal::dma::Event::TransferError);
        self.tx_dma.enable();

        // TX channel always finishes before the RX.
        // The transfer completion is controlled by RX channel event.
        while !self
            .rx_dma
            .event_occurred(hal::dma::Event::TransferComplete)
        {}
        self.tx_dma.disable();
        self.rx_dma.disable();
        Ok(buf)
    }
}
