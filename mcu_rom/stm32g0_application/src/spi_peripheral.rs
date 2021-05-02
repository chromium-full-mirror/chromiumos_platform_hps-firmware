// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hal::dma::Channel;
use hal::dma::Event::TransferError;
use mcu_common::MemBlock;
use stm32g0::stm32g071::SPI1;

const BUFFER_SIZE: usize = 64;

/// Main buffer. Used for receving, then sending, but never at the same time.
static mut BUFFER: [u8; BUFFER_SIZE] = [0u8; BUFFER_SIZE];

/// The SPI interface with the FPGA. We act as the peripheral on the SPI bus,
/// with the FPGA as the controller.
pub struct SpiPeripheral {
    spi: SPI1,
    tx_dma: hal::dma::C1,
    rx_dma: hal::dma::C2,
    mode: Mode,
    cs_state: bool,
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
            // Ignore the SPI clock signal for now. We stop ignoring it only
            // when the CS line goes low, which we handle in `cs_changed`. Note,
            // setting SSI=1 is like pulling the CS line low, which deselects
            // the device.
            w.ssi().set_bit();
            // Use software, not hardware to control SSI.
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

        // TODO: Enable interrupts for DMA errors.

        // safety: The following MemBlock takes ownership of its mutable
        // reference. This is safe provided each it has exactly one owner, which
        // we can guarantee because we take ownership of SPI1, so cannot be
        // called more than once.
        let buffer = MemBlock::new(unsafe { &mut BUFFER });

        let mut spi_peripheral = Self {
            spi,
            tx_dma,
            rx_dma,
            mode: Mode::Receive(buffer),
            cs_state: true,
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
            defmt::error!("DMA transfer error");
        }
    }

    pub(crate) fn handle_tx_dma_interrupt(&mut self) {
        if self.tx_dma.event_occurred(TransferError) {
            // TODO: Report error properly.
            defmt::error!("DMA transfer error");
        }
    }

    pub(crate) fn cs_changed(&mut self, cs: bool) -> Option<MemBlock> {
        self.spi.cr1.modify(|_, w| w.ssi().bit(cs));
        self.cs_state = cs;
        let mut result = None;
        if cs {
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
                // zeros while we receive and in subseuquent states until we
                // reach the Send state.
                self.spi.dr.write(|w| w);
                self.spi.dr.write(|w| w);
                configure_dma_channel(
                    &mut self.rx_dma,
                    hal::dmamux::DmaMuxIndex::SPI1_RX,
                    hal::dma::Direction::FromPeripheral,
                    buffer,
                );
            }
            Mode::Send(buffer) => {
                configure_dma_channel(
                    &mut self.tx_dma,
                    hal::dmamux::DmaMuxIndex::SPI1_TX,
                    hal::dma::Direction::FromMemory,
                    buffer,
                );
            }
            _ => {}
        }
    }
}

fn configure_dma_channel(
    channel: &mut impl hal::dma::Channel,
    peripheral: hal::dmamux::DmaMuxIndex,
    direction: hal::dma::Direction,
    buffer: &MemBlock,
) {
    let spi1_registers = unsafe { &*hal::stm32::SPI1::ptr() };
    let spi_dr_address = &spi1_registers.dr as *const _ as u32;

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
