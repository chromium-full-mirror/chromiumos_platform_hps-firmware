// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

pub mod boards;
mod camera;
mod debug;
mod spi_flash;
mod spi_peripheral;

use crate::boards::board;
use crate::spi_peripheral::SpiPeripheral;
use application::fpga;
use application::ApplicationState;
use application::HostInterface;
use hal::prelude::*;
use hal::timer::stopwatch::Stopwatch;
use hal::timer::Timer;
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
        fpga_power_gate: board::FpgaPowerGate,
        fpga_up_channel: UpChannel,
        image_channel: UpChannel,
        mcu_cmd_response_channel: UpChannel,
        fpga_debug_handler: debug::DebugCommandHandler,
        mcu_debug_handler: debug::DebugCommandHandler,
        spi_flash: Option<Flash<board::FlashSpi, board::FlashSpiCs>>,
        timer: Timer<hal::stm32::TIM16>,
        camera_i2c: board::CameraI2c,
        stopwatch: Stopwatch<hal::stm32::TIM2>,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    #[task(binds = I2C1, resources = [host_interface, state], spawn = [write_spi_flash], priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        match ctx
            .resources
            .host_interface
            .handle_interrupt(ctx.resources.state)
        {
            Some(application::Event::Reset) => cortex_m::peripheral::SCB::sys_reset(),
            Some(application::Event::WriteSpiFlash(write_request)) => {
                let _ = ctx.spawn.write_spi_flash(write_request);
            }
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
        ctx.resources.exti.unpend(board::FPGA_CS_EVENT);
    }

    #[task(binds = USART2, resources = [host_interface, state], priority = 3)]
    fn handle_usart2(ctx: handle_usart2::Context) {
        ctx.resources
            .host_interface
            .handle_interrupt(ctx.resources.state);
    }

    #[task(binds = TIM16,
        resources = [mcu_debug_handler, fpga_programn, timer, camera_i2c, spi_flash, stopwatch,
            mcu_cmd_response_channel, fpga_power_gate])]
    fn timer_tick(ctx: timer_tick::Context) {
        ctx.resources.timer.clear_irq();
        while let Some(command) = ctx.resources.mcu_debug_handler.get_mcu_debug_command() {
            let mut handled = true;
            if board::SPI_FLASH_ENABLED {
                match command.kind {
                    McuDebugCommand::ReadSpiFlash => {
                        ctx.resources.fpga_programn.set_low().unwrap();
                        spi_flash::debug_read(ctx.resources.spi_flash, command.arg as u32)
                    }
                    McuDebugCommand::WriteSpiFlash => {
                        if let Some(mut write_request) = command.write_request {
                            ctx.resources.fpga_programn.set_low().unwrap();
                            spi_flash::write(
                                ctx.resources.spi_flash,
                                write_request.address,
                                &mut *write_request.data,
                            );
                            ctx.resources.mcu_cmd_response_channel.write(&[0, 0, 0, 0]);
                        }
                    }
                    McuDebugCommand::FpgaHoldInReset => {
                        ctx.resources.fpga_programn.set_low().unwrap()
                    }
                    McuDebugCommand::EraseSpiFlash => {
                        ctx.resources.fpga_programn.set_low().unwrap();
                        spi_flash::erase(ctx.resources.spi_flash);
                    }
                    McuDebugCommand::HashSpiFlash => {
                        ctx.resources.fpga_programn.set_low().unwrap();
                        spi_flash::hash(
                            ctx.resources.spi_flash,
                            command.arg,
                            ctx.resources.stopwatch,
                        )
                    }
                    McuDebugCommand::SpiFlashCrc => {
                        ctx.resources.fpga_programn.set_low().unwrap();
                        ctx.resources
                            .mcu_cmd_response_channel
                            .write(&spi_flash::crc_sector(ctx.resources.spi_flash, command.arg));
                    }
                    McuDebugCommand::SpiFlashReadSpeed => {
                        ctx.resources.fpga_programn.set_low().unwrap();
                        spi_flash::test_read_speed(
                            ctx.resources.spi_flash,
                            ctx.resources.stopwatch,
                        );
                    }
                    McuDebugCommand::SpiFlashWriteSpeed => {
                        ctx.resources.fpga_programn.set_low().unwrap();
                        spi_flash::test_write_speed(
                            ctx.resources.spi_flash,
                            ctx.resources.stopwatch,
                        );
                    }
                    McuDebugCommand::FpgaPowerOff => {
                        ctx.resources.fpga_power_gate.set_high().unwrap();
                        rprintln!("FPGA and SPI flash powered off");
                    }
                    McuDebugCommand::FpgaPowerOn => {
                        ctx.resources.fpga_power_gate.set_low().unwrap();
                        rprintln!("FPGA and SPI flash powered on");
                    }
                    _ => {
                        handled = false;
                    }
                }
            }
            match command.kind {
                McuDebugCommand::ResetFpga => reset_fpga(ctx.resources.fpga_programn),
                McuDebugCommand::ReadCameraId => camera::read_camera_id(ctx.resources.camera_i2c),
                McuDebugCommand::Ping => rprintln!("Pong!"),
                _ => {
                    if !handled {
                        rprintln!("Unsupported MCU debug command");
                    }
                }
            }
        }
    }

    #[task(resources = [spi, state, debug_led, fpga_programn, fpga_up_channel,
        image_channel, fpga_debug_handler, mcu_debug_handler, camera_i2c])]
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
            fpga::Action::ReadCameraI2c => {
                let num_addresses: usize = buffer[1] as usize;
                camera::read_camera_reg(
                    ctx.resources.camera_i2c,
                    // Buffer is a sequence of one or more 2B addresses.
                    &mut buffer[2..(num_addresses * 2 + 2)],
                );
            }
            fpga::Action::WriteCameraI2c => {
                let num_addresses: usize = buffer[1] as usize;
                camera::write_camera_reg(
                    ctx.resources.camera_i2c,
                    // Buffer is a sequence of one or more 2B addresses
                    // and 1B values.
                    &mut buffer[2..(num_addresses * 3 + 3)],
                );
            }
            fpga::Action::None => {}
        }
        request.prepare_to_send(&mut buffer);
        ctx.resources.spi.lock(|spi| spi.send(buffer));
    }

    #[task(resources = [spi_flash, fpga_programn])]
    fn write_spi_flash(ctx: write_spi_flash::Context, mut request: i2c_protocol::WriteMemoryEvent) {
        // Put FPGA into reset while we write SPI flash. We don't currently take
        // it out of reset. That's should be done once we've signature-checked
        // it.
        if board::SPI_FLASH_ENABLED {
            ctx.resources.fpga_programn.set_low().unwrap();
            spi_flash::write(ctx.resources.spi_flash, request.address, &mut *request.data);
        }
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
            3: {
                size: 128
                mode: NoBlockSkip
                name: "McuCmdResponses"
            }
        }
        down: {
            0: {
                size: 4096
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

    let i2c_config = stm32g0_i2c_peripheral::Config {
        clock_stretching: !cfg!(feature = "no-i2c-clock-stretching"),
    };

    let mut board = board::Board::new(
        p.GPIOA.split(&mut rcc),
        p.GPIOB.split(&mut rcc),
        p.GPIOC.split(&mut rcc),
        p.I2C1,
        p.I2C2,
        p.USART2,
        p.SPI1,
        p.SPI2,
        &mut rcc,
        i2c_config,
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

    let stopwatch = p.TIM2.stopwatch(&mut rcc);

    let camera_i2c = board.camera_i2c.unwrap();

    // Ensure FPGA and SPI flash are powered.
    board.fpga_power_gate.set_low().unwrap();

    // Put the FPGA into reset, otherwise we won't be able to reliably access
    // the SPI flash.
    board.fpga_programn.set_low().unwrap();

    let mut spi_flash = None;

    if let (Some(flash_spi), Some(flash_spi_cs)) = (board.flash_spi, board.flash_spi_cs) {
        // Enable quad SPI. This setting is non-volatile, so in theory we only
        // need do this once, however the cost of doing it every boot is only a
        // single read, which is very cheap.
        let mut f = spi_flash::SpiFlash {
            spi: flash_spi,
            cs: flash_spi_cs,
        };
        if f.enable_quad_spi().is_err() {
            rprintln!("Failed to enable quad SPI");
        }
        let flash_spi = f.spi;
        let flash_spi_cs = f.cs;

        match Flash::init(flash_spi, flash_spi_cs) {
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
    }

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
        fpga_power_gate: board.fpga_power_gate,
        fpga_up_channel: channels.up.1,
        image_channel: channels.up.2,
        mcu_cmd_response_channel: channels.up.3,
        mcu_debug_handler: debug::DebugCommandHandler::new(channels.down.0),
        fpga_debug_handler: debug::DebugCommandHandler::new(channels.down.1),
        spi_flash,
        timer,
        camera_i2c,
        stopwatch,
    }
}
