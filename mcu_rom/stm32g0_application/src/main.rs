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
use mcu_common::MemBlock;
use panic_probe as _;
use rtic::app;
use stm32g0_i2c_peripheral::Stm32g0I2cPeripheral;

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        host_interface: HostInterface<Stm32g0I2cPeripheral>,
        spi: SpiPeripheral,
        exti: hal::stm32::EXTI,
        spi_cs: hal::gpio::gpiob::PB0<hal::gpio::Input<hal::gpio::Floating>>,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    #[task(binds = I2C1, resources = [host_interface], priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        match ctx.resources.host_interface.handle_interrupt() {
            Some(application::Event::Reset) => cortex_m::peripheral::SCB::sys_reset(),
            None => {}
        }
    }

    #[task(binds = DMA_CHANNEL1, resources = [spi], priority = 2)]
    fn spi_tx_dma_interrupt(ctx: spi_tx_dma_interrupt::Context) {
        ctx.resources.spi.handle_tx_dma_interrupt();
    }

    #[task(binds = DMA_CHANNEL2_3, resources = [spi], priority = 2)]
    fn spi_rx_dma_interrupt(ctx: spi_rx_dma_interrupt::Context) {
        ctx.resources.spi.handle_rx_dma_interrupt();
    }

    /// Interrupt handler for EXTI channels 0 through 1.
    #[task(binds = EXTI0_1, resources = [spi, exti, spi_cs], spawn = [handle_spi_packet],
        priority = 2)]
    fn exti_event(ctx: exti_event::Context) {
        // Currently the only event configured to use this interrupt handler is
        // a change in PB0, the SPI CS line, so we don't bother checking which
        // EXTI interrupt is pending.
        ctx.resources.exti.unpend(hal::exti::Event::GPIO0);
        if let Some(buffer) = ctx
            .resources
            .spi
            .cs_changed(ctx.resources.spi_cs.is_high().unwrap_or_default())
        {
            // This should never fail since there's only one buffer and we have
            // it, so the `handle_spi_packet` task's queue can't have it.
            let _ = ctx.spawn.handle_spi_packet(buffer);
        }
    }

    #[task(resources = [spi])]
    fn handle_spi_packet(mut ctx: handle_spi_packet::Context, mut buffer: MemBlock) {
        // TODO: Do something useful with the data we received. For now we
        // just change the data that was sent to us then send it back.
        for v in buffer.iter_mut() {
            *v = 255 - *v;
        }
        ctx.resources.spi.lock(|spi| spi.send(buffer));
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
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
    let mut exti = p.EXTI;

    let host_interface = HostInterface::new(Stm32g0I2cPeripheral::new(
        p.I2C1,
        &mut rcc,
        gpiob.pb9.into_open_drain_output(),
        gpiob.pb8.into_open_drain_output(),
        mcu_common::HPS_ADDRESS,
    ));

    let spi = SpiPeripheral::new(p.SPI1, gpioa.pa5, gpioa.pa6, gpioa.pa7, dma.ch1, dma.ch2);

    // Trigger an interrupt when the SPI CS line changes.
    let spi_cs = gpiob.pb0.listen(hal::gpio::SignalEdge::All, &mut exti);

    // Set PB0 to use alternate function 0.
    //
    // TODO: See if we can add support to the HAL crate for doing this.
    // Currently the alternate function code is private and exposed via
    // functions such as `PinSck::setup`.
    unsafe {
        let gpiob = &*stm32g0::stm32g071::GPIOB::ptr();
        gpiob.afrl.modify(|_, w| w.afsel0().bits(0));
        gpiob.moder.modify(|_, w| w.moder0().bits(2));
    }

    defmt::info!(
        "MCU application started. CPU is running at {}Mhz",
        rcc.clocks.sys_clk.0 / 1000000
    );
    init::LateResources {
        host_interface,
        spi,
        exti,
        spi_cs,
    }
}
