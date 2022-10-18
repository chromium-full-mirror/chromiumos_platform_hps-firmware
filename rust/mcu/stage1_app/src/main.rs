// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

pub mod boards;
mod camera;
#[cfg(feature = "dev")]
mod debug;
mod fpga_device;
mod header;
mod panic_handler;
mod spi_dma_controller;
mod spi_flash;
mod spi_peripheral;
mod testing;

use crate::boards::board;
use crate::fpga_device::get_spi_flash;
use crate::fpga_device::FpgaDevice;
use crate::spi_peripheral::SpiPeripheral;
use application::fpga;
use application::ApplicationState;
use application::HostInterface;
use core::convert::TryFrom;
use hal::prelude::*;
use hal::rcc::Rcc;
use hal::stm32::SYST;
use hal::time::MicroSecond;
use hal::timer::delay::Delay;
use hal::timer::pwm::PwmPin;
use hal::timer::stopwatch::Stopwatch;
use log::error;
use log::info;
use mcu_common::commands::Command;
use mcu_common::Buffer;
use mcu_common::Error;
use mcu_common::MemBlock;
use mcu_common::PartIds;
use mcu_common::Status;
use spi_memory::series25::Flash;

const CAMERA_ID: u16 = 0x01b0;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

#[rtic::app(
    device = hal::stm32,
    peripherals = true,
    dispatchers = [TIM17])]
mod app {
    use super::*;

    #[shared]
    struct SharedResources {
        state: ApplicationState,
        host_interface: HostInterface<board::HostBus, board::InterruptPin>,
        spi: SpiPeripheral,
        #[lock_free]
        exti: hal::stm32::EXTI,
        #[lock_free]
        debug_led: board::DebugLed,
        #[lock_free]
        fpga_device: FpgaDevice,
        fpga_timer: hal::timer::Timer<hal::stm32::TIM15>,
        #[cfg(feature = "dev")]
        #[lock_free]
        debug_resources: crate::debug::DebugResources,
        #[cfg(feature = "dev")]
        #[lock_free]
        debug_timer: hal::timer::Timer<hal::stm32::TIM16>,
        #[lock_free]
        camera_i2c: board::CameraI2c,
        #[lock_free]
        stopwatch: Stopwatch<hal::stm32::TIM2>,
        #[lock_free]
        tick_count: u32,
        #[lock_free]
        rcc: Rcc,
        #[lock_free]
        frame_timer: hal::timer::Timer<hal::stm32::TIM14>,
        #[lock_free]
        pwm_channel: PwmPin<hal::stm32::TIM3, hal::timer::Channel4>,
        #[lock_free]
        delay: Delay<SYST>,
    }

    #[local]
    struct LocalResources {}

    #[init(local = [
        i2c_recv_buffer: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE],
        fpga_buffer: [u8; fpga::BUFFER_SIZE] = [0; fpga::BUFFER_SIZE],
        #[cfg(feature = "dev")]
        debug_recv_buffer: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE],
    ])]
    fn init(ctx: init::Context) -> (SharedResources, LocalResources, init::Monotonics) {
        let p = ctx.device;
        let cp = ctx.core;
        let mut init_status = Status::empty();
        match p.FLASH.optr.read().rdp().bits() {
            0xaa => {}
            0xcc => init_status |= Status::STAGE0_PERM_LOCKED,
            _ => init_status |= Status::STAGE0_LOCKED,
        }
        // safety: We're calling this during initialization when interrupts are
        // disabled as required.
        #[allow(unused_variables, unused_mut)]
        let mut debug_resources = unsafe { debug::DebugResources::init() };

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
        let mut rcc = p.RCC.freeze(rcc_low_freq_config());
        // These clock values are set manually due to stm32g0xx-hal bug
        // (ref. https://github.com/stm32-rs/stm32g0xx-hal/issues/110).
        rcc.clocks.apb_clk = 12.mhz();
        rcc.clocks.apb_tim_clk = 12.mhz();
        let mut delay = Delay::syst(cp.SYST, &rcc);

        let i2c_config = create_i2c_config(&rcc);

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

        let mut host_interface =
            HostInterface::new(board.host_bus, board.interrupt_pin, &header::__IMAGE_HDR);
        host_interface.init_status = init_status;

        // We either use our MemBlock to store a crash that occurred on the previous
        // boot, or if there was no crash, we supply the MemBlock to the host
        // interface in order to receive data.
        let mut buffer = Buffer::new(MemBlock::new(ctx.local.i2c_recv_buffer));
        panic_handler::copy_crash_report(&mut buffer);
        if !buffer.is_empty() {
            host_interface.report_previous_crash(buffer);
        } else {
            host_interface.supply_mem_block(buffer.release());
        }

        // Set up a timer for detecting if the FPGA stops sending us updates
        // because it has crashed/hung. The timer interval is chosen to be
        // larger than the duration of each iteration of the FPGA's main loop.
        // We reset the timer whenever we receive an update from the FPGA, so if
        // the timer interrupt fires we know something is wrong.
        let fpga_timer_duration: MicroSecond = 5_000.ms().into();
        let mut fpga_timer = p.TIM15.timer(&mut rcc);
        fpga_timer.start(fpga_timer_duration);

        // Trigger an interrupt when the SPI CS line changes.
        let fpga_spi_cs = board
            .fpga_spi_cs
            .listen(hal::gpio::SignalEdge::All, &mut exti);

        let spi = SpiPeripheral::new(
            board.fpga_spi,
            board.fpga_spi_clk,
            board.fpga_spi_cipo,
            board.fpga_spi_copi,
            fpga_spi_cs,
            dma.ch1,
            dma.ch2,
            board.spi_ready,
            MemBlock::new(ctx.local.fpga_buffer),
        );

        let stopwatch = p.TIM2.stopwatch(&mut rcc);

        let pwm = board.camera_mclk_tim.pwm(6.mhz(), &mut rcc);
        let mut pwm_channel = pwm.bind_pin(board.camera_mclk.set_speed(hal::gpio::Speed::VeryHigh));
        pwm_channel.set_duty((pwm_channel.get_max_duty() + 1) / 2);

        // Power off the FPGA, since if there is non-standard gateware loaded, it
        // may ignore reset.
        board.fpga_power_gate.set_high().unwrap();

        // Put the FPGA into reset, otherwise we won't be able to reliably access
        // the SPI flash.
        board.fpga_programn.set_low().unwrap();

        // Wait a bit to make sure that the FPGA has really powered off before we
        // power it back on.
        delay.delay(25.ms());

        let flash_spi = spi_dma_controller::SpiDmaController::new(
            board.flash_spi,
            board.flash_spi_clk,
            board.flash_spi_cipo,
            board.flash_spi_copi,
            dma.ch3,
            dma.ch4,
        );
        let mut spi_flash = None;

        // Ensure FPGA and SPI flash are powered.
        board.fpga_power_gate.set_low().unwrap();

        // Give the SPI flash chip a chance to start after we powered it on.
        // Experimentally, on one non-dev device, 40ms was insufficient, but 50ms
        // was sufficient. So we give it double-that to be sure.
        delay.delay(100.ms());

        let mut f = spi_flash::SpiFlash {
            spi: flash_spi,
            cs: board.flash_spi_cs,
        };

        if let Ok(status) = f.read_status() {
            host_interface.spi_flash_status = status;
        }

        let flash_spi = f.spi;
        let flash_spi_cs = f.cs;

        let mut part_ids = PartIds::default();

        part_ids.mcu_id = p.DBG.idcode.read().bits();

        match Flash::init(flash_spi, flash_spi_cs) {
            Ok(mut flash) => match flash.read_jedec_id() {
                Ok(id) => {
                    part_ids.spi_flash_manufacturer_id = id.mfr_code() as u32;
                    part_ids.set_spi_flash_device_id(id.device_id());
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

        let camera_i2c = board.camera_i2c;
        let mut frame_timer = p.TIM14.timer(&mut rcc);
        frame_timer.listen();

        info!(
            "MCU application started. CPU is running at {}MHz",
            rcc.clocks.ahb_clk.0 / 1000000
        );

        host_interface.part_ids = part_ids;

        let fpga_device = FpgaDevice::new(board.fpga_programn, board.fpga_power_gate, spi_flash);

        // If something tried to address us while we were still initializing, the
        // bus might have ended up in a bad state, reset it.
        host_interface.reset_bus();

        #[cfg(feature = "dev")]
        {
            debug_resources
                .mcu_debug_handler
                .supply_mem_block(MemBlock::new(ctx.local.debug_recv_buffer));
        }

        let shared = SharedResources {
            host_interface,
            spi,
            exti,
            state: ApplicationState::default(),
            debug_led: board.debug_led,
            fpga_device,
            fpga_timer,
            #[cfg(feature = "dev")]
            debug_resources,
            #[cfg(feature = "dev")]
            debug_timer: debug::init_debug_timer(p.TIM16, &mut rcc),
            camera_i2c,
            frame_timer,
            stopwatch,
            tick_count: 0,
            rcc,
            pwm_channel,
            delay,
        };
        let local = LocalResources {};

        (shared, local, init::Monotonics())
    }

    fn create_i2c_config(rcc: &Rcc) -> stm32g0_i2c_peripheral::Config {
        let mut i2c_config = stm32g0_i2c_peripheral::Config::new(
            stm32g0_i2c_peripheral::Address::new_7bit(mcu_common::HPS_ADDRESS),
            stm32g0_i2c_peripheral::Timing::from_speed(rcc, 400.khz()),
        );
        i2c_config.clock_stretching = !cfg!(feature = "no-i2c-clock-stretching");
        i2c_config
    }

    #[task(binds = I2C1, shared = [host_interface, state],
        priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        let mut host_interface = ctx.shared.host_interface;
        let event = (&mut host_interface, ctx.shared.state).lock(|h, s| h.handle_i2c_events(s));
        match event {
            Some(application::Event::Command(Command::Reset)) => {
                cortex_m::peripheral::SCB::sys_reset()
            }
            Some(application::Event::Command(Command::EraseSpiFlash)) => {
                host_interface.lock(|h| h.set_spi_flash_erasing(true));
                let _ = erase_spi_flash::spawn();
            }
            Some(application::Event::WriteSpiFlash(write_request)) => {
                let _ = write_spi_flash::spawn(write_request);
            }
            Some(application::Event::Command(Command::WriteSpiFlashTestData)) => {
                let _ = write_spi_flash_test_data::spawn();
            }
            #[cfg(feature = "dev")]
            Some(application::Event::Command(Command::MlbInterrupt)) => {
                let _ = signal_interrupt::spawn();
            }
            Some(application::Event::Command(Command::LaunchApp)) => {
                // We can safely ignore failed attempts to swap this task
                // because 1 attempt to start the FPGA is equivalent to 2.
                let _ = try_start_fpga::spawn();
            }
            Some(application::Event::Command(Command::TriggerMcuPanic)) => {
                panic!("Intentional panic");
            }
            Some(application::Event::Command(command)) => {
                error!("Unsupported command {:?}", command);
                host_interface.lock(|h| {
                    h.report_error(mcu_common::Error::HostI2cBadRequest);
                    h.command_completed();
                });
            }
            Some(application::Event::TestCameraI2c) => {
                let _ = test_camera_i2c::spawn();
            }
            None => {}
        }
    }

    #[task(binds = DMA_CHANNEL1, shared = [spi], priority = 2)]
    fn spi_tx_dma_interrupt(mut ctx: spi_tx_dma_interrupt::Context) {
        ctx.shared.spi.lock(|spi| spi.handle_tx_dma_interrupt());
    }

    #[task(binds = DMA_CHANNEL2_3, shared = [spi], priority = 2)]
    fn spi_rx_dma_interrupt(mut ctx: spi_rx_dma_interrupt::Context) {
        ctx.shared.spi.lock(|spi| spi.handle_rx_dma_interrupt());
    }

    /// Interrupt handler for EXTI channels 4 through 15. Currently only used
    /// for FPGA SPI CS changes.
    #[task(binds = EXTI4_15, shared = [spi, exti, fpga_timer],
        priority = 2)]
    fn exti4_15_event(mut ctx: exti4_15_event::Context) {
        // It's essential that we unpend the CS transition event before we call
        // cs_changed, since cs_changed might change the state of the READY pin,
        // which the FPGA ROM might be waiting on, resulting in another CS
        // transition event to occur straight away. If we unpend after
        // cs_changed, we might miss that subsequent CS transition.
        ctx.shared.exti.unpend(board::FPGA_CS_EVENT);
        if let Some(buffer) = ctx.shared.spi.lock(|spi| spi.cs_changed()) {
            ctx.shared.fpga_timer.lock(|timer| timer.reset());
            // This should never fail since there's only one buffer and we have
            // it, so the `handle_spi_packet` task's queue can't have it.
            let _ = handle_spi_packet::spawn(buffer);
        }
    }

    #[task(binds = TIM14, shared = [frame_timer, camera_i2c, host_interface])]
    fn tim14_tick(mut ctx: tim14_tick::Context) {
        let timer = ctx.shared.frame_timer;
        timer.pause();
        timer.clear_irq();

        let mut camera = hm01b0::Camera::new(ctx.shared.camera_i2c);
        if camera.set_mode(hm01b0::Mode::StreamingNFrames(1)).is_err() {
            ctx.shared
                .host_interface
                .lock(|host_interface| host_interface.report_error(mcu_common::Error::CameraI2c));
        }
    }

    #[task(binds = TIM15, shared = [fpga_timer, host_interface])]
    fn fpga_timer_tick(mut ctx: fpga_timer_tick::Context) {
        ctx.shared.fpga_timer.lock(|fpga_timer| {
            fpga_timer.clear_irq();
            // Once a timeout has happened, there is no reason to keep trying to detect further
            // timeouts.
            fpga_timer.unlisten();
        });
        ctx.shared
            .host_interface
            .lock(|host_interface| host_interface.report_error(mcu_common::Error::FpgaTimeout));
        error!("FPGA timeout expired");
    }

    // Ideally this whole task would be excluded when the dev feature is
    // disabled, however this fails to compile (at least with rtic 0.5.9).
    // Presumably it's trying to include the function in the interrupt table
    // when it doesn't exist.
    #[task(binds = TIM16,
        shared = [debug_resources, fpga_device, debug_timer, camera_i2c, stopwatch,
            debug_led, tick_count, host_interface])]
    fn debug_timer_tick(mut ctx: debug_timer_tick::Context) {
        // Make "use" of ctx to prevent unused variable warning when dev feature
        // is off.
        let _ = &mut ctx;
        #[cfg(feature = "dev")]
        {
            ctx.shared.debug_timer.clear_irq();
            let led = &mut ctx.shared.debug_led;
            *ctx.shared.tick_count = (*ctx.shared.tick_count).wrapping_add(1);
            if *ctx.shared.tick_count & 16 == 0 {
                led.set_low().unwrap();
            } else {
                led.set_high().unwrap();
            }
            while let Some(command) = ctx
                .shared
                .debug_resources
                .mcu_debug_handler
                .get_mcu_debug_command()
            {
                debug::handle_mcu_debug_command(command, &mut ctx.shared);
            }
        }
    }

    #[task(shared = [camera_i2c, state, host_interface])]
    fn test_camera_i2c(mut ctx: test_camera_i2c::Context) {
        let mut camera = hm01b0::Camera::new(ctx.shared.camera_i2c);
        loop {
            // For each camera test iteration, we perform 100 reads of the
            // camera ID.
            for _ in 0..100 {
                if !camera
                    .get_camera_id()
                    .map(|id| id == CAMERA_ID)
                    .unwrap_or(false)
                {
                    ctx.shared.host_interface.lock(|host_interface| {
                        host_interface.report_error(mcu_common::Error::CameraI2c)
                    });
                    break;
                }
            }
            // Decrement test iteration register and break out of loop if it has
            // reached zero.
            if ctx.shared.state.lock(|state| {
                state.camera_test_iterations = state.camera_test_iterations.saturating_sub(1);
                state.camera_test_iterations == 0
            }) {
                break;
            }
        }
    }

    #[task(shared = [spi, state, debug_led, fpga_device, fpga_timer, debug_resources,
        camera_i2c, host_interface, frame_timer])]
    fn handle_spi_packet(mut ctx: handle_spi_packet::Context, mut buffer: MemBlock) {
        let request = fpga::Request::from_received_packet(&buffer);
        match &request.action {
            fpga::Action::SetDebugLed => {
                if buffer[1] == 0 {
                    ctx.shared.debug_led.set_low().unwrap();
                } else {
                    ctx.shared.debug_led.set_high().unwrap();
                }
            }
            fpga::Action::ReadConfigRegisters => ctx
                .shared
                .state
                .lock(|state| fpga::read_config_registers(&mut buffer, state)),
            fpga::Action::WriteStatusRegisters => {
                ctx.shared.state.lock(|state| {
                    fpga::write_status_registers(&buffer, state);
                    ctx.shared
                        .host_interface
                        .lock(|h| h.report_status_update(state));
                });
            }
            fpga::Action::ReportFpgaBoot => {
                let count = ctx
                    .shared
                    .state
                    .lock(|state| fpga::report_boot(state, &buffer));
                info!("FPGA reported boot #{}", count);
                ctx.shared.fpga_timer.lock(|t| {
                    t.reset();
                    t.clear_irq();
                    t.listen();
                });
            }
            fpga::Action::ReportFpgaPanic => {
                ctx.shared
                    .host_interface
                    .lock(|h| h.report_fpga_panic(&buffer[1..]));
            }
            #[cfg(feature = "dev")]
            fpga::Action::DebugLog => {
                if let Some(message) = fpga::payload_as_string(&buffer) {
                    let channel = &mut ctx.shared.debug_resources.fpga_up_channel;
                    use core::fmt::Write;
                    write!(channel, "{}", message).unwrap();
                } else {
                    info!("FPGA sent invalid message");
                }
            }
            #[cfg(feature = "image-transfer")]
            fpga::Action::DebugImage => {
                if !debug::transfer_image(&buffer, &mut ctx.shared) {
                    // Keep repeating this task until the I2C buffer becomes
                    // available and the data can be sent.
                    let _ = handle_spi_packet::spawn(buffer);
                    return;
                }
            }
            #[cfg(feature = "dev")]
            fpga::Action::GetDebugCommand => {
                ctx.shared
                    .debug_resources
                    .fpga_debug_handler
                    .get_debug_command(&mut buffer[2..]);
            }
            fpga::Action::ReadCameraI2c => {
                let num_addresses: usize = buffer[1] as usize;
                camera::read_camera_reg(
                    ctx.shared.camera_i2c,
                    // Buffer is a sequence of one or more 2B addresses.
                    &mut buffer[2..(num_addresses * 2 + 2)],
                );
            }
            fpga::Action::WriteCameraI2c => {
                let num_addresses: usize = buffer[1] as usize;
                camera::write_camera_reg(
                    ctx.shared.camera_i2c,
                    // Buffer is a sequence of one or more 2B addresses
                    // and 1B values.
                    &mut buffer[2..(num_addresses * 3 + 3)],
                );
            }
            fpga::Action::ReportFpgaError => {
                let error_bits = u16::from_le_bytes([buffer[1], buffer[2]]);
                if let Ok(error) = mcu_common::Error::try_from(error_bits) {
                    info!("FPGA reports error: {:?}", error);
                    ctx.shared.host_interface.lock(|h| h.report_error(error));
                }
            }
            fpga::Action::TriggerFrame => {
                // We set a delay of at least 1us, since a delay of 0 isn't
                // permitted (HAL asserts > 0).
                let frame_delay = MicroSecond(
                    u32::from_le_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]).max(1),
                );
                ctx.shared.frame_timer.start(frame_delay);
            }
            fpga::Action::Poll => {}
            fpga::Action::Echo => {}
        }
        request.prepare_to_send(&mut buffer);
        ctx.shared.spi.lock(|spi| spi.send(buffer));
    }

    #[task(shared = [fpga_device, host_interface])]
    fn erase_spi_flash(mut ctx: erase_spi_flash::Context) {
        if let Err(error) = spi_flash::erase(get_spi_flash(ctx.shared.fpga_device).ok()) {
            ctx.shared
                .host_interface
                .lock(|host_interface| host_interface.report_error(error));
        }
        ctx.shared
            .host_interface
            .lock(|host_interface| host_interface.set_spi_flash_erasing(false));
        ctx.shared
            .host_interface
            .lock(|host_interface| host_interface.command_completed());
    }

    #[task(shared = [fpga_device, host_interface])]
    fn write_spi_flash(mut ctx: write_spi_flash::Context, request: i2c_protocol::WriteMemoryEvent) {
        if let Err(error) = spi_flash::write(
            get_spi_flash(ctx.shared.fpga_device).ok(),
            request.address,
            &*request.data,
        ) {
            ctx.shared
                .host_interface
                .lock(|host_interface| host_interface.report_error(error));
        }
        ctx.shared
            .host_interface
            .lock(|host_interface| host_interface.supply_mem_block(request.data.release()));
    }

    #[task(shared = [fpga_device, host_interface, state, rcc, pwm_channel, camera_i2c, delay])]
    fn try_start_fpga(ctx: try_start_fpga::Context) {
        let mut host_interface = ctx.shared.host_interface;
        // Switch System Clock to HSI (required to reconfigure PLL).
        let raw_rcc = unsafe { hal::stm32::Peripherals::steal().RCC };
        raw_rcc.cfgr.write(|w| w);
        let mut rcc = raw_rcc.freeze(rcc_high_freq_config());
        // Reconfigure host I2C, since we've changed the clock configuration.
        let i2c_config = create_i2c_config(&rcc);
        host_interface.lock(|h| h.i2c_mut().reinitialize(&mut rcc, i2c_config));
        match ctx.shared.fpga_device.start_fpga(ctx.shared.rcc) {
            Ok(_) => {
                host_interface.lock(|h| h.set_application_running(true));
            }
            Err(error) => {
                error!("Unable to start FPGA: {:?}", error);
                host_interface.lock(|host_interface| host_interface.report_error(error));
            }
        }

        // Switch System Clock to HSI (required to reconfigure PLL).
        let raw_rcc = unsafe { hal::stm32::Peripherals::steal().RCC };
        raw_rcc.cfgr.write(|w| w);
        let mut rcc = raw_rcc.freeze(rcc_low_freq_config());
        // Reconfigure host I2C, since we've changed the clock configuration.
        let i2c_config = create_i2c_config(&rcc);
        host_interface.lock(|h| h.i2c_mut().reinitialize(&mut rcc, i2c_config));

        // Now that we're done messing with clock frequencies, we can turn on
        // the camera MCLK then configure the camera.
        ctx.shared.pwm_channel.enable();
        match initialize_camera(ctx.shared.camera_i2c, ctx.shared.delay) {
            Ok(id) => host_interface.lock(|h| h.part_ids.camera_id = id.into()),
            Err(error) => {
                error!("Failed to initialize camera: {error:?}");
                host_interface.lock(|h| h.report_error(error));
            }
        }

        host_interface.lock(|h| h.command_completed());
    }

    /// Initialize the camera, returning the camera's ID if successful.
    fn initialize_camera(
        camera_i2c: &mut board::CameraI2c,
        delay: &mut Delay<SYST>,
    ) -> Result<u16, Error> {
        let mut camera = hm01b0::Camera::new(camera_i2c);
        // Occasionally the first read from the camera fails, so we ignore it.
        let _ = camera.get_camera_id();
        let id = camera.get_camera_id()?;
        if id != CAMERA_ID {
            info!("Camera reported unexpected ID: {}", id);
            return Err(Error::CameraUnexpectedId);
        }
        camera.run_init_script(delay)?;
        Ok(id)
    }

    #[task(shared = [fpga_device, host_interface])]
    fn write_spi_flash_test_data(ctx: write_spi_flash_test_data::Context) {
        let mut host_interface = ctx.shared.host_interface;
        if let Err(error) = get_spi_flash(ctx.shared.fpga_device)
            .and_then(|spi_flash| testing::write_spi_test_data(spi_flash))
        {
            host_interface.lock(|h| h.report_error(error));
        }
        host_interface.lock(|h| h.command_completed());
    }

    #[task(shared = [host_interface])]
    fn signal_interrupt(mut ctx: signal_interrupt::Context) {
        ctx.shared.host_interface.lock(|h| {
            h.assert_interrupt();
        });
        // TODO: If we use signal_interrupt for something other than testing and
        // debugging purposes, we should use a timer rather than a delay so that
        // other tasks at this priory can run concurrently. Also, basing the
        // delay on cycles means that changes in our CPU speed will affect the
        // duration. But this is good enough for testing purposes. If the CPU is
        // running at slow mode (12 MHz), then 100_000 cycles should be about 8ms.
        cortex_m::asm::delay(100_000);
        ctx.shared.host_interface.lock(|h| {
            h.deassert_interrupt();
            h.command_completed();
        });
    }
}

fn rcc_low_freq_config() -> hal::rcc::Config {
    // RCC configuration for a low-power mode.
    // SYSCLK is running at 48 MHz. Core, AHB, APB clock is running at 12 MHz.
    // SYSCLK (a multiple of 12 MHz) is required in order to drive the camera at 6 MHz.
    let pll_cfg = hal::rcc::PllConfig {
        m: 1,
        n: 12,
        r: 4,
        ..hal::rcc::PllConfig::default()
    };

    hal::rcc::Config::pll()
        .pll_cfg(pll_cfg)
        .ahb_psc(hal::rcc::Prescaler::Div4)
}

fn rcc_high_freq_config() -> hal::rcc::Config {
    // RCC configuration for a high-power mode.
    // SYSCLK is running at 64 MHz. Core, AHB, APB clock is running at 64 MHz.
    // This configuration is only used during the flash verification stage.
    let pll_cfg = hal::rcc::PllConfig {
        m: 1,
        n: 16,
        r: 4,
        ..hal::rcc::PllConfig::default()
    };

    hal::rcc::Config::pll().pll_cfg(pll_cfg)
}

// We could automatically enable the dev feature when image-transfer is enabled,
// but we want the bar for enabling image-transfer to be higher.
#[cfg(all(not(feature = "dev"), feature = "image-transfer"))]
compile_error!("image-transfer feature requires dev feature");

// Unfortunately RTIC needs types to be defined even if they're used by resource
// fields that aren't compiled due to conditional compilation.
#[cfg(not(feature = "dev"))]
mod debug {
    pub struct DebugResources;

    impl DebugResources {
        pub unsafe fn init() {}
    }
}
