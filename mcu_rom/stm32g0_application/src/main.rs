// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

mod spi_peripheral;

use crate::spi_peripheral::SpiPeripheral;
use application::HostInterface;
use defmt_rtt as _;
use hal::prelude::*;
use panic_probe as _;
use rtic::app;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        host_interface: HostInterface<Stm32g0I2cPeripheral>,
        spi: SpiPeripheral,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    #[task(binds = I2C1, resources = [host_interface])]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        match ctx.resources.host_interface.handle_interrupt() {
            Some(application::Event::Reset) => cortex_m::peripheral::SCB::sys_reset(),
            None => {}
        }
    }

    #[task(binds = DMA_CHANNEL1, resources = [spi])]
    fn spi_tx_dma_interrupt(ctx: spi_tx_dma_interrupt::Context) {
        ctx.resources.spi.handle_tx_dma_interrupt();
    }

    #[task(binds = DMA_CHANNEL2_3, resources = [spi])]
    fn spi_rx_dma_interrupt(ctx: spi_rx_dma_interrupt::Context) {
        ctx.resources.spi.handle_rx_dma_interrupt();
    }
};

fn rcc_config() -> hal::rcc::Config {
    hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default())
}

fn init_peripherals(p: hal::stm32::Peripherals) -> init::LateResources {
    // Enable SPI peripheral.
    p.RCC.apbenr2.write(|w| w.spi1en().set_bit());
    // Reset SPI peripheral.
    p.RCC.apbrstr2.write(|w| w.spi1rst().set_bit());
    p.RCC.apbrstr2.write(|w| w.spi1rst().clear_bit());
    // Enable DMA clock so that RTT (the mechanism used to send defmt messages)
    // is still handled when we're idle and the CPU is put to sleep.
    p.RCC.ahbenr.modify(|_, w| w.dmaen().set_bit());
    let mut rcc = p.RCC.freeze(rcc_config());

    let gpioa = p.GPIOA.split(&mut rcc);
    let gpiob = p.GPIOB.split(&mut rcc);
    let dma = p.DMA.split(&mut rcc, p.DMAMUX);

    let host_interface = HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
        mcu_common::HPS_ADDRESS,
    ));

    let spi = SpiPeripheral::new(p.SPI1, gpioa.pa5, gpioa.pa6, gpioa.pa7, dma.ch1, dma.ch2);

    defmt::info!(
        "MCU application started. CPU is running at {}Mhz",
        rcc.clocks.sys_clk.0 / 1000000
    );
    init::LateResources {
        host_interface,
        spi,
    }
}
