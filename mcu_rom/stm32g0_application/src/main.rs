// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

pub mod boards;
mod debug;
mod spi_peripheral;

use core::convert::TryFrom;

use crate::boards::board;
use crate::spi_peripheral::SpiPeripheral;
use application::fpga;
use application::ApplicationState;
use application::HostInterface;
use hal::prelude::*;
use mcu_common::McuDebugCommand;
use mcu_common::MemBlock;
use panic_rtt_target as _;
use rtic::app;
use rtt_target::rprintln;
use rtt_target::rtt_init;
use rtt_target::UpChannel;
use spi_memory::series25::Flash;

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        state: ApplicationState,
        host_interface: HostInterface<board::HostBus>,
        spi: SpiPeripheral,
        exti: hal::stm32::EXTI,
        debug_led: board::DebugLed,
        fpga_programn: board::FpgaProgramn,
        fpga_up_channel: UpChannel,
        image_channel: UpChannel,
        fpga_debug_handler: debug::DebugCommandHandler,
        mcu_debug_handler: debug::DebugCommandHandler,
        spi_flash: Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
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

    #[task(binds = TIM16, resources = [mcu_debug_handler, fpga_programn])]
    fn timer_tick(ctx: timer_tick::Context) {
        let mut cmd_buf = [0u8; 3];
        ctx.resources
            .mcu_debug_handler
            .get_debug_command(&mut cmd_buf);
        if cmd_buf[0] != 0 {
            match McuDebugCommand::try_from(cmd_buf[0]) {
                Ok(command) => match command {
                    McuDebugCommand::ResetFpga => reset_fpga(ctx.resources.fpga_programn),
                },
                Err(_) => rprintln!("Unrecognized debug command {}", cmd_buf[0]),
            }
        }
    }

    #[task(resources = [spi, state, debug_led, fpga_programn, fpga_up_channel,
        image_channel, fpga_debug_handler, mcu_debug_handler])]
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
                    let channel = ctx.resources.fpga_up_channel;
                    use core::fmt::Write;
                    write!(channel, "{}", message).unwrap();
                } else {
                    rprintln!("FPGA sent invalid message");
                }
            }
            fpga::Action::DebugImage => {
                ctx.resources.image_channel.write(&buffer[1..]);
            }
            fpga::Action::GetDebugCommand => {
                ctx.resources
                    .fpga_debug_handler
                    .get_debug_command(&mut buffer[2..]);
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
    rprintln!("Resetting FPGA");
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
    let channels = rtt_init!(
        up: {
            0: {
                size: 256
                mode: NoBlockSkip
                name: "MCU"
            }
            1: {
                size: 256
                mode: NoBlockSkip
                name: "FPGA"
            }
            2: {
                size: 1024
                mode: NoBlockSkip
                name: "Images"
            }
        }
        down: {
            0: {
                size: 16
                mode: NoBlockSkip
                name: "McuCommands"
            }
            1: {
                size: 16
                mode: NoBlockSkip
                name: "FpgaCommands"
            }
        }
    );
    rtt_target::set_print_channel(channels.up.0);
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
        p.SPI1,
        p.SPI2,
        &mut rcc,
    );
    let dma = p.DMA.split(&mut rcc, p.DMAMUX);
    let mut exti = p.EXTI;

    let host_interface = HostInterface::new(board.host_bus);

    // Set up a periodic timer. This is only needed for polling our debug
    // channels, so won't be needed in production.
    let mut timer = p.TIM16.timer(&mut rcc);
    timer.start(10.hz());
    timer.listen();

    // Trigger an interrupt when the SPI CS line changes.
    let spi_cs = board
        .fpga_spi_cs
        .listen(hal::gpio::SignalEdge::All, &mut exti);

    let spi = SpiPeripheral::new(
        board.fpga_spi,
        board.fpga_spi_clk,
        board.fpga_spi_cipo,
        board.fpga_spi_copi,
        spi_cs,
        dma.ch1,
        dma.ch2,
        board.spi_ready,
    );

    let mut spi_flash = None;

    match Flash::init(board.flash_spi, board.flash_spi_cs) {
        Ok(mut flash) => match flash.read_jedec_id() {
            Ok(id) => {
                if id.mfr_code() == board::SPI_FLASH_MANUFACTURER_ID
                    && id.device_id() == board::SPI_FLASH_DEVICE_ID
                {
                    rprintln!("Found expected flash chip");
                    spi_flash = Some(flash);
                } else {
                    rprintln!(
                        "Unexpected flash ID {}, [{}, {}]",
                        id.mfr_code(),
                        id.device_id()[0],
                        id.device_id()[1]
                    );
                }
            }
            Err(_) => rprintln!("Failed to read flash ID"),
        },
        Err(e) => {
            rprintln!("Failed to initialize SPI flash: {:?}", e);
        }
    };

    rprintln!(
        "MCU application started. CPU is running at {}MHz",
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
        fpga_up_channel: channels.up.1,
        image_channel: channels.up.2,
        mcu_debug_handler: debug::DebugCommandHandler::new(channels.down.0),
        fpga_debug_handler: debug::DebugCommandHandler::new(channels.down.1),
        spi_flash,
    }
}
