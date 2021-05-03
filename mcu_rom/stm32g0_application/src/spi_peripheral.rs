// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hal::dma::Channel;
use hal::dma::Event::TransferComplete;
use hal::dma::Event::TransferError;
use stm32g0::stm32g071::SPI1;

/// The SPI interface with the FPGA. We act as the peripheral on the SPI bus,
/// with the FPGA as the controller.
pub struct SpiPeripheral {
    tx_dma: hal::dma::C1,
    rx_dma: hal::dma::C2,
}

impl SpiPeripheral {
    pub(crate) fn new(
        spi: SPI1,
        sck: impl hal::spi::PinSck<SPI1>,
        miso: impl hal::spi::PinMiso<SPI1>,
        mosi: impl hal::spi::PinMosi<SPI1>,
        tx_dma: hal::dma::C1,
        rx_dma: hal::dma::C2,
    ) -> SpiPeripheral {
        hal::spi::PinSck::setup(&sck);
        hal::spi::PinMiso::setup(&miso);
        hal::spi::PinMosi::setup(&mosi);

        spi.cr1.write(|w| {
            // We're the only peripheral on this bus, assume we're always active and
            // ignore CS line.
            // TODO: Make use of the CS line to frame our data.
            w.ssm().set_bit()
        });

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

        let mut spi_peripheral = Self { tx_dma, rx_dma };
        spi_peripheral.configure_rx_dma();
        spi_peripheral.configure_tx_dma();
        spi_peripheral.tx_dma.listen(TransferComplete);
        spi_peripheral.rx_dma.listen(TransferComplete);

        // Enable DMA for transmit.
        spi.cr2.modify(|_, w| w.txdmaen().set_bit());
        // Enable SPI.
        spi.cr1.modify(|_, w| w.spe().set_bit());

        spi_peripheral
    }

    pub(crate) fn handle_rx_dma_interrupt(&mut self) {
        if self.rx_dma.event_occurred(TransferComplete) {
            self.rx_dma.clear_event(TransferComplete);
            // TODO: Transfer ownership of buffer and handle buffer at a lower
            // interrupt priority.
            defmt::info!("Received bytes: {:?}", unsafe { &SPI_BUFFERS.recv });
            self.configure_rx_dma();
        }
        if self.rx_dma.event_occurred(TransferError) {
            // TODO: Report error properly.
            defmt::error!("DMA transfer error");
        }
    }

    pub(crate) fn handle_tx_dma_interrupt(&mut self) {
        if self.tx_dma.event_occurred(TransferComplete) {
            self.tx_dma.clear_event(TransferComplete);
            self.configure_tx_dma();
        }
        if self.tx_dma.event_occurred(TransferError) {
            // TODO: Report error properly.
            defmt::error!("DMA transfer error");
        }
    }

    fn configure_rx_dma(&mut self) -> () {
        let buffer = unsafe { &SPI_BUFFERS.recv };
        configure_dma_channel(
            &mut self.rx_dma,
            hal::dmamux::DmaMuxIndex::SPI1_RX,
            hal::dma::Direction::FromPeripheral,
            buffer,
        );
    }

    fn configure_tx_dma(&mut self) -> () {
        let buffer = unsafe { &SPI_BUFFERS.send };
        configure_dma_channel(
            &mut self.tx_dma,
            hal::dmamux::DmaMuxIndex::SPI1_TX,
            hal::dma::Direction::FromMemory,
            buffer,
        );
    }
}

fn configure_dma_channel(
    channel: &mut impl hal::dma::Channel,
    peripheral: hal::dmamux::DmaMuxIndex,
    direction: hal::dma::Direction,
    buffer: &'static [u8],
) {
    let spi1_registers = unsafe { &*hal::stm32::SPI1::ptr() };
    let spi_dr_address = &spi1_registers.dr as *const _ as u32;

    channel.disable();
    channel.set_direction(direction);
    channel.set_memory_address(buffer.as_ptr() as u32, true);
    channel.set_transfer_length(buffer.len() as u16);
    channel.set_peripheral_address(spi_dr_address, false);
    // Note, the type parameter to `set_word_size` seems to be unused.
    // TODO: See if we can clean this up in the HAL crate.
    channel.set_word_size::<usize>(hal::dma::WordSize::BITS8);
    channel.select_peripheral(peripheral);
    channel.enable();
}

// TODO: Increase buffer size to 64 bytes and set up double-buffering, with
// buffer ownership transferred to code that makes use of those buffers.
#[repr(align(16))]
struct SpiBuffers {
    send: [u8; 16],
    recv: [u8; 16],
}

static mut SPI_BUFFERS: SpiBuffers = SpiBuffers {
    send: [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128],
    recv: [0u8; 16],
};
