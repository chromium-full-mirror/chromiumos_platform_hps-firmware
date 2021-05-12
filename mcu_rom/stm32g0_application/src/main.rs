// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

pub mod boards;
mod spi_peripheral;

use crate::boards::board;
use crate::spi_peripheral::SpiPeripheral;
use application::fpga;
use application::ApplicationState;
use application::HostInterface;
use defmt_rtt as _;
use hal::prelude::*;
use mcu_common::MemBlock;
use panic_probe as _;
use rtic::app;

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        state: ApplicationState,
        host_interface: HostInterface<board::HostBus>,
        spi: SpiPeripheral,
        exti: hal::stm32::EXTI,
        debug_led: board::DebugLed,
        fpga_programn: board::FpgaProgramn,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    #[task(binds = I2C1, resources = [host_interface, state], priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        match ctx
            .resources
            .host_interface
            .handle_interrupt(ctx.resources.state)
        {
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

    /// Interrupt handler for EXTI channels 0 through 1. Currently only used for
    /// FPGA SPI CS changes.
    #[task(binds = EXTI0_1, resources = [spi, exti], spawn = [handle_spi_packet],
        priority = 2)]
    fn exti0_1_event(ctx: exti0_1_event::Context) {
        if let Some(buffer) = ctx.resources.spi.cs_changed() {
            // This should never fail since there's only one buffer and we have
            // it, so the `handle_spi_packet` task's queue can't have it.
            let _ = ctx.spawn.handle_spi_packet(buffer);
        }
        ctx.resources.exti.unpend(hal::exti::Event::GPIO0);
    }

    /// Interrupt handler for EXTI channels 4 through 15. Currently only used
    /// for FPGA SPI CS changes.
    #[task(binds = EXTI4_15, resources = [spi, exti], spawn = [handle_spi_packet],
        priority = 2)]
    fn exti4_15_event(ctx: exti4_15_event::Context) {
        if let Some(buffer) = ctx.resources.spi.cs_changed() {
            // This should never fail since there's only one buffer and we have
            // it, so the `handle_spi_packet` task's queue can't have it.
            let _ = ctx.spawn.handle_spi_packet(buffer);
        }
        ctx.resources.exti.unpend(hal::exti::Event::GPIO4);
    }

    #[task(binds = USART2, resources = [host_interface, state], priority = 3)]
    fn handle_usart2(ctx: handle_usart2::Context) {
        ctx.resources
            .host_interface
            .handle_interrupt(ctx.resources.state);
    }

    #[task(resources = [spi, state, debug_led, fpga_programn])]
    fn handle_spi_packet(mut ctx: handle_spi_packet::Context, mut buffer: MemBlock) {
        let request = fpga::Request::from_received_packet(&buffer);
        match &request.action {
            fpga::Action::SetDebugLed(true) => {
                ctx.resources.debug_led.set_high().unwrap();
            }
            fpga::Action::SetDebugLed(false) => {
                ctx.resources.debug_led.set_low().unwrap();
            }
            fpga::Action::ResetFpga => reset_fpga(ctx.resources.fpga_programn),
            fpga::Action::ReadConfigRegisters => ctx
                .resources
                .state
                .lock(|state| fpga::read_config_registers(&mut buffer, state)),
            fpga::Action::WriteStatusRegisters => ctx
                .resources
                .state
                .lock(|state| fpga::write_status_registers(&buffer, state)),
            fpga::Action::DebugLog => {
                if let Some(message) = fpga::payload_as_string(&buffer) {
                    defmt::info!("FPGA says: {}", message);
                } else {
                    defmt::error!("FPGA sent invalid message");
                }
            }
            fpga::Action::None => {}
        }
        request.prepare_to_send(&mut buffer);
        ctx.resources.spi.lock(|spi| spi.send(buffer));
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
    }
};

fn reset_fpga(fpga_programn: &mut board::FpgaProgramn) {
    defmt::info!("Resetting FPGA");
    // Hold programn low for long enough to reset the FPGA. Experimentally 2
    // iterations is sufficient, so we do 4 just to be sure.
    for _ in 0..4 {
        fpga_programn.set_low().unwrap();
    }
    fpga_programn.set_high().unwrap();
}

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

    let mut board = board::Board::new(
        p.GPIOA.split(&mut rcc),
        p.GPIOB.split(&mut rcc),
        p.GPIOC.split(&mut rcc),
        p.I2C1,
        p.USART2,
        &mut rcc,
    );
    let dma = p.DMA.split(&mut rcc, p.DMAMUX);
    let mut exti = p.EXTI;

    let host_interface = HostInterface::new(board.host_bus);

    // Trigger an interrupt when the SPI CS line changes.
    let spi_cs = board
        .fpga_spi_cs
        .listen(hal::gpio::SignalEdge::All, &mut exti);

    let spi = SpiPeripheral::new(
        p.SPI1,
        board.fpga_spi_clk,
        board.fpga_spi_cipo,
        board.fpga_spi_copi,
        spi_cs,
        dma.ch1,
        dma.ch2,
    );

    defmt::info!(
        "MCU application started. CPU is running at {}Mhz",
        rcc.clocks.sys_clk.0 / 1000000
    );
    // Reset the FPGA on startup so that it's in a known state.
    reset_fpga(&mut board.fpga_programn);
    init::LateResources {
        host_interface,
        spi,
        exti,
        state: ApplicationState::default(),
        debug_led: board.debug_led,
        fpga_programn: board.fpga_programn,
    }
}
