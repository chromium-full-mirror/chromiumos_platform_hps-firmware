// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

pub mod boards;
mod camera;
mod debug;
mod fpga_device;
mod header;
mod spi_flash;
mod spi_peripheral;

use crate::boards::board;
use crate::fpga_device::{get_spi_flash, FpgaDevice};
use crate::spi_peripheral::SpiPeripheral;
use application::fpga;
use application::ApplicationState;
use application::HostInterface;
use debug::McuDebugCommandInfo;
use hal::delay::Delay;
use hal::prelude::*;
use hal::timer::stopwatch::Stopwatch;
use hal::timer::Timer;
use log::info;
use mcu_common::McuDebugCommand;
use mcu_common::MemBlock;
use panic_rtt_target as _;
use rtic::app;
use rtt_target::rtt_init;
use rtt_target::UpChannel;
use spi_memory::series25::Flash;

const CAMERA_ID: u16 = 0x01b0;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

#[app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        state: ApplicationState,
        host_interface: HostInterface<board::HostBus>,
        spi: SpiPeripheral,
        exti: hal::stm32::EXTI,
        debug_led: board::DebugLed,
        fpga_device: FpgaDevice,
        fpga_up_channel: UpChannel,
        image_channel: UpChannel,
        mcu_cmd_response_channel: UpChannel,
        fpga_debug_handler: debug::DebugCommandHandler,
        mcu_debug_handler: debug::DebugCommandHandler,
        timer: Timer<hal::stm32::TIM16>,
        camera_i2c: board::CameraI2c,
        stopwatch: Stopwatch<hal::stm32::TIM2>,
        tick_count: u32,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        static mut I2C_RECV_BUFFER: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE];
        static mut DEBUG_RECV_BUFFER: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE];
        let mut resources = init_peripherals(ctx.device, ctx.core);
        resources
            .host_interface
            .supply_mem_block(MemBlock::new(I2C_RECV_BUFFER));
        resources
            .mcu_debug_handler
            .supply_mem_block(MemBlock::new(DEBUG_RECV_BUFFER));
        resources
    }

    #[task(binds = I2C1, resources = [host_interface, state],
        spawn = [write_spi_flash, try_start_fpga], priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        let host_interface = ctx.resources.host_interface;
        match host_interface.handle_interrupt(ctx.resources.state) {
            Some(application::Event::Reset) => cortex_m::peripheral::SCB::sys_reset(),
            Some(application::Event::WriteSpiFlash(write_request)) => {
                let _ = ctx.spawn.write_spi_flash(write_request);
            }
            Some(application::Event::StartFpga) => {
                // We can safely ignore failed attempts to swap this task
                // because 1 attempt to start the FPGA is equivalent to 2.
                let _ = ctx.spawn.try_start_fpga();
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

    #[task(binds = TIM16,
        resources = [mcu_debug_handler, fpga_device, timer, camera_i2c, stopwatch,
            mcu_cmd_response_channel, debug_led, tick_count])]
    fn timer_tick(mut ctx: timer_tick::Context) {
        ctx.resources.timer.clear_irq();
        let led = &mut ctx.resources.debug_led;
        *ctx.resources.tick_count = (*ctx.resources.tick_count).wrapping_add(1);
        if *ctx.resources.tick_count & 16 == 0 {
            led.set_low().unwrap();
        } else {
            led.set_high().unwrap();
        }
        while let Some(command) = ctx.resources.mcu_debug_handler.get_mcu_debug_command() {
            handle_mcu_debug_command(command, &mut ctx.resources);
        }
    }

    #[task(resources = [spi, state, debug_led, fpga_device, fpga_up_channel,
        image_channel, fpga_debug_handler, mcu_debug_handler, camera_i2c, host_interface],
        spawn = [handle_spi_packet])]
    fn handle_spi_packet(mut ctx: handle_spi_packet::Context, mut buffer: MemBlock) {
        let request = fpga::Request::from_received_packet(&buffer);
        match &request.action {
            fpga::Action::SetDebugLed(true) => {
                ctx.resources.debug_led.set_high().unwrap();
            }
            fpga::Action::SetDebugLed(false) => {
                ctx.resources.debug_led.set_low().unwrap();
            }
            fpga::Action::ResetFpga => {
                ctx.resources.fpga_device.reset_fpga();
            }
            fpga::Action::ReadConfigRegisters => ctx
                .resources
                .state
                .lock(|state| fpga::read_config_registers(&mut buffer, state)),
            fpga::Action::WriteStatusRegisters => ctx
                .resources
                .state
                .lock(|state| fpga::write_status_registers(&buffer, state)),
            fpga::Action::ReportFpgaBoot => {
                let count = ctx
                    .resources
                    .state
                    .lock(|state| fpga::report_boot(state, &buffer));
                info!("FPGA reported boot #{}", count);
            }
            fpga::Action::DebugLog => {
                if let Some(message) = fpga::payload_as_string(&buffer) {
                    let channel = ctx.resources.fpga_up_channel;
                    use core::fmt::Write;
                    write!(channel, "{}", message).unwrap();
                } else {
                    info!("FPGA sent invalid message");
                }
            }
            #[cfg(feature = "image-transfer")]
            fpga::Action::DebugImage => {
                if !transfer_image(
                    &buffer,
                    ctx.resources.image_channel,
                    ctx.resources.state,
                    ctx.resources.host_interface,
                ) {
                    // Keep repeating this task until the I2C buffer becomes
                    // available and the data can be sent.
                    let _ = ctx.spawn.handle_spi_packet(buffer);
                    return;
                }
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

    #[task(resources = [fpga_device, host_interface, mcu_debug_handler])]
    fn write_spi_flash(mut ctx: write_spi_flash::Context, request: i2c_protocol::WriteMemoryEvent) {
        if let Err(error) = spi_flash::write(
            get_spi_flash(ctx.resources.fpga_device),
            request.address,
            &*request.data,
        ) {
            ctx.resources
                .host_interface
                .lock(|host_interface| host_interface.report_error(error));
        }
        ctx.resources
            .host_interface
            .lock(|host_interface| host_interface.supply_mem_block(request.data.release()));
    }

    #[task(resources = [fpga_device, host_interface])]
    fn try_start_fpga(mut ctx: try_start_fpga::Context) {
        ctx.resources.fpga_device.start_fpga();
        ctx.resources
            .host_interface
            .lock(|h| h.set_application_running(true));
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
    }
};

#[cfg(feature = "image-transfer")]
fn transfer_image(
    buffer: &MemBlock,
    image_channel: &mut UpChannel,
    mut state: resources::state,
    mut host_interface: resources::host_interface,
) -> bool {
    use rtic::Mutex;
    let i2c_image_xfer_enabled = state.lock(|s| s.i2c_image_transfer_enabled());
    let mut success = true;
    if i2c_image_xfer_enabled {
        host_interface.lock(|host_interface| {
            success = host_interface.queue_image_data_for_i2c(&buffer[1..]);
        });
    } else {
        image_channel.write(&buffer[1..]);
    }
    success
}

fn handle_mcu_debug_command(command: McuDebugCommandInfo, resources: &mut timer_tick::Resources) {
    let mut handled = true;
    match command.kind {
        McuDebugCommand::ReadSpiFlash => {
            spi_flash::debug_read(get_spi_flash(resources.fpga_device), command.arg as u32)
        }
        McuDebugCommand::WriteSpiFlash => {
            if let Some(write_request) = command.write_request {
                // For now, we ignore returned errors here, since the write
                // function will have printed the error message in more detail
                // already.
                let _ = spi_flash::write(
                    get_spi_flash(resources.fpga_device),
                    write_request.address,
                    &*write_request.data,
                );
                resources
                    .mcu_debug_handler
                    .supply_mem_block(write_request.data.release());
                resources.mcu_cmd_response_channel.write(&[0, 0, 0, 0]);
            }
        }
        McuDebugCommand::FpgaHoldInReset => resources.fpga_device.hold.set_low().unwrap(),
        McuDebugCommand::EraseSpiFlash => {
            spi_flash::erase(get_spi_flash(resources.fpga_device));
        }
        McuDebugCommand::HashSpiFlash => spi_flash::hash(
            get_spi_flash(resources.fpga_device),
            command.arg,
            resources.stopwatch,
        ),
        McuDebugCommand::SpiFlashCrc => {
            resources
                .mcu_cmd_response_channel
                .write(&spi_flash::crc_sector(
                    get_spi_flash(resources.fpga_device),
                    command.arg,
                ));
        }
        McuDebugCommand::SpiFlashReadSpeed => {
            spi_flash::test_read_speed(get_spi_flash(resources.fpga_device), resources.stopwatch);
        }
        McuDebugCommand::SpiFlashWriteSpeed => {
            spi_flash::test_write_speed(get_spi_flash(resources.fpga_device), resources.stopwatch);
        }
        McuDebugCommand::FpgaPowerOff => {
            resources.fpga_device.power_gate.set_high().unwrap();
            info!("FPGA and SPI flash powered off");
        }
        McuDebugCommand::FpgaPowerOn => {
            resources.fpga_device.power_gate.set_low().unwrap();
            info!("FPGA and SPI flash powered on");
        }
        _ => {
            handled = false;
        }
    }
    match command.kind {
        McuDebugCommand::ResetFpga => {
            resources.fpga_device.reset_fpga();
        }
        McuDebugCommand::ReadCameraId => camera::read_camera_id(resources.camera_i2c),
        McuDebugCommand::ReadCameraAEConfig => camera::read_camera_ae_config(resources.camera_i2c),
        McuDebugCommand::ReadCameraIntegrationGains => {
            camera::read_camera_gains(resources.camera_i2c);
            camera::read_camera_integration(resources.camera_i2c);
        }
        McuDebugCommand::SetCameraDigitalGain => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match c.set_digital_gain(command.arg) {
                Ok(_) => camera::read_camera_gains(resources.camera_i2c),
                Err(_) => info!("Failed to write digital gain"),
            }
        }
        McuDebugCommand::SetCameraAnalogGain => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match c.set_analog_gain(command.arg) {
                Ok(_) => camera::read_camera_gains(resources.camera_i2c),
                Err(_) => info!("Failed to write analog gain"),
            }
        }
        McuDebugCommand::SetCameraIntegration => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            info!("Writing {}", command.arg);
            match c.set_integration(command.arg) {
                Ok(_) => camera::read_camera_integration(resources.camera_i2c),
                Err(_) => info!("Failed to write integration"),
            }
        }
        McuDebugCommand::Ping => info!("Pong!"),
        _ => {
            if !handled {
                info!("Unsupported MCU debug command");
            }
        }
    }
}

fn rcc_config() -> hal::rcc::Config {
    // Clock our CPU at 60MHz. This is as close as we can get to the maximum
    // supported clock speed of 64MHz, while still being a multiple of 12MHz,
    // which is required in order to drive the camera at 6MHz. Once we have the
    // camera's clock connected to a pin supported by TIM1, then we can if we
    // want separate these and run the CPU at the full 64MHz.

    // Changing this back to 64MHz is blocked on an update to stm32g0xx-hal. See
    // TODO in proto2.rs regarding camera_mclk_tim being TIM3 instead of TIM1.
    let pll_cfg = hal::rcc::PllConfig {
        m: 1,
        n: 15,
        r: 4,
        ..hal::rcc::PllConfig::default()
    };

    hal::rcc::Config::pll().pll_cfg(pll_cfg)
}

fn init_peripherals(
    p: hal::stm32::Peripherals,
    cp: rtic::export::Peripherals,
) -> init::LateResources {
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
                mode: BlockIfFull
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
                // This must be at least large enough to fit 256 *
                // MAX_OUTSTANDING_COMMANDS (as defined in hps-mon).
                size: 8192
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
    // safety: We're calling this during initialization when interrupts are
    // disabled as required.
    unsafe { debug_logger::initialize_logging() };
    // Reset clock source to HSI. This is necessary because the bootloader
    // (either stage0 or the STM32 system bootloader) will have left the clock
    // source configured as the PLL.
    p.RCC.cfgr.write(|w| w);
    // Configure our vector table. This is necessary when being loaded by a
    // bootloader. Neither stage0 nor the STM32 system bootloader will do this
    // for us.
    unsafe { cp.SCB.vtor.write(&VECTOR_TABLE as *const _ as u32) };
    // Enable SPI peripheral.
    p.RCC.apbenr2.write(|w| w.spi1en().set_bit());
    p.RCC.apbenr1.write(|w| w.spi2en().set_bit());
    // Reset SPI peripheral.
    p.RCC.apbrstr2.write(|w| w.spi1rst().set_bit());
    p.RCC.apbrstr2.write(|w| w.spi1rst().clear_bit());
    p.RCC.apbrstr1.write(|w| w.spi2rst().set_bit());
    p.RCC.apbrstr1.write(|w| w.spi2rst().clear_bit());
    // Enable DMA clock so that RTT (the mechanism used to send defmt messages)
    // is still handled when we're idle and the CPU is put to sleep.
    p.RCC.ahbenr.modify(|_, w| w.dmaen().set_bit());
    let mut rcc = p.RCC.freeze(rcc_config());
    let mut delay = Delay::syst(cp.SYST, &rcc);

    let mut i2c_config = stm32g0_i2c_peripheral::Config::new(
        stm32g0_i2c_peripheral::Address::new_7bit(mcu_common::HPS_ADDRESS),
        stm32g0_i2c_peripheral::Timing::from_speed(&mut rcc, 400.khz()),
    );
    i2c_config.clock_stretching = !cfg!(feature = "no-i2c-clock-stretching");

    let mut board = board::Board::new(
        p.GPIOA.split(&mut rcc),
        p.GPIOB.split(&mut rcc),
        p.GPIOC.split(&mut rcc),
        p.I2C1,
        p.I2C2,
        p.SPI1,
        p.SPI2,
        p.TIM1,
        p.TIM3,
        &mut rcc,
        i2c_config,
    );
    let dma = p.DMA.split(&mut rcc, p.DMAMUX);
    let mut exti = p.EXTI;

    let mut host_interface = HostInterface::new(board.host_bus);

    // Set up a periodic timer. This is only needed for polling our debug
    // channels, so won't be needed in production. 100Hz is chosen in order to
    // maximize the speed of writing to SPI flash from hps-mon. Going faster
    // than 100Hz gives little to no gain in write speed.
    let mut timer = p.TIM16.timer(&mut rcc);
    timer.start(100.hz());
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

    let mut camera_i2c = board.camera_i2c.unwrap();

    let pwm = board.camera_mclk_tim.pwm(6.mhz(), &mut rcc);
    let mut pwm_channel = pwm.bind_pin(board.camera_mclk.set_speed(hal::gpio::Speed::VeryHigh));
    pwm_channel.set_duty((pwm_channel.get_max_duty() + 1) / 2);
    pwm_channel.enable();

    // Put the FPGA into reset, otherwise we won't be able to reliably access
    // the SPI flash.
    board.fpga_programn.set_low().unwrap();

    let mut spi_flash = None;

    if let (Some(flash_spi), Some(mut flash_spi_cs)) = (board.flash_spi, board.flash_spi_cs) {
        // CS line must be set high before we power on the SPI flash.
        flash_spi_cs.set_high().unwrap();

        // Ensure FPGA and SPI flash are powered.
        board.fpga_power_gate.set_low().unwrap();

        // Give the SPI flash chip a chance to start after we powered it on. The
        // datasheet says minimum time is 2.5ms. To be generous, we give it
        // double that.
        delay.delay(5.ms());

        let mut f = spi_flash::SpiFlash {
            spi: flash_spi,
            cs: flash_spi_cs,
        };

        if let Ok(status) = f.read_status() {
            host_interface.spi_flash_status = status;
        }

        let flash_spi = f.spi;
        let flash_spi_cs = f.cs;

        match Flash::init(flash_spi, flash_spi_cs) {
            Ok(mut flash) => match flash.read_jedec_id() {
                Ok(id) => {
                    if id.mfr_code() == board::SPI_FLASH_MANUFACTURER_ID
                        && id.device_id() == board::SPI_FLASH_DEVICE_ID
                    {
                        info!("Found expected flash chip");

                        // Enable quad SPI. This setting is non-volatile, so in
                        // theory we only need do this once, however the cost of
                        // doing it every boot is only a single read, which is
                        // very cheap.
                        if spi_flash::enable_quad_spi(&mut flash).is_err() {
                            host_interface.report_error(mcu_common::Error::SpiFlash);
                            info!("Failed to enable quad SPI");
                        }

                        spi_flash = Some(flash);
                    } else {
                        host_interface.report_error(mcu_common::Error::SpiFlash);
                        info!(
                            "Unexpected flash ID {}, [{}, {}]",
                            id.mfr_code(),
                            id.device_id()[0],
                            id.device_id()[1]
                        );
                    }
                }
                Err(_) => {
                    host_interface.report_error(mcu_common::Error::SpiFlash);
                    info!("Failed to read flash ID");
                }
            },
            Err(e) => {
                host_interface.report_error(mcu_common::Error::SpiFlash);
                info!("Failed to initialize SPI flash: {:?}", e);
            }
        };
    }

    match hm01b0::Camera::new(&mut camera_i2c).get_camera_id() {
        Err(_) => {
            host_interface.report_error(mcu_common::Error::Camera);
            info!("Failed to read camera ID");
        }
        Ok(CAMERA_ID) => {}
        Ok(id) => {
            host_interface.report_error(mcu_common::Error::Camera);
            info!("Camera reported unexpected ID: {}", id);
        }
    }

    info!(
        "MCU application started. CPU is running at {}MHz",
        rcc.clocks.sys_clk.0 / 1000000
    );

    let fpga_device = FpgaDevice::new(board.fpga_programn, board.fpga_power_gate, spi_flash);

    init::LateResources {
        host_interface,
        spi,
        exti,
        state: ApplicationState::default(),
        debug_led: board.debug_led,
        fpga_device,
        fpga_up_channel: channels.up.1,
        image_channel: channels.up.2,
        mcu_cmd_response_channel: channels.up.3,
        mcu_debug_handler: debug::DebugCommandHandler::new(channels.down.0),
        fpga_debug_handler: debug::DebugCommandHandler::new(channels.down.1),
        timer,
        camera_i2c,
        stopwatch,
        tick_count: 0,
    }
}
