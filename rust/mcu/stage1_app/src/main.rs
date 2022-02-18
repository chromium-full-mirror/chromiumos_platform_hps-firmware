// Copyright 2021 The Chromium OS Authors. All rights reserved.
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
mod spi_dma_controller;
mod spi_flash;
mod spi_peripheral;
mod testing;

use crate::boards::board;
use crate::fpga_device::{get_spi_flash, FpgaDevice};
use crate::spi_peripheral::SpiPeripheral;
use application::fpga;
use application::ApplicationState;
use application::HostInterface;
use hal::delay::Delay;
use hal::prelude::*;
use hal::rcc::Rcc;
use hal::timer::stopwatch::Stopwatch;
use log::error;
use log::info;
use mcu_common::commands::Command;
use mcu_common::MemBlock;
use mcu_common::PartIds;
use mcu_common::Status;
#[cfg(not(feature = "dev"))]
use panic_halt as _;
#[cfg(feature = "dev")]
use panic_rtt_target as _;
use rtic::app;
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
        #[cfg(feature = "dev")]
        debug_resources: crate::debug::DebugResources,
        #[cfg(feature = "dev")]
        timer: hal::timer::Timer<hal::stm32::TIM16>,
        camera_i2c: board::CameraI2c,
        stopwatch: Stopwatch<hal::stm32::TIM2>,
        tick_count: u32,
        rcc: Rcc,
        interrupt_pin: board::InterruptPin,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        static mut I2C_RECV_BUFFER: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE];
        #[cfg(feature = "dev")]
        static mut DEBUG_RECV_BUFFER: [u8; i2c_protocol::RECV_BUFFER_SIZE] =
            [0; i2c_protocol::RECV_BUFFER_SIZE];
        let mut resources = init_peripherals(ctx.device, ctx.core);
        resources
            .host_interface
            .supply_mem_block(MemBlock::new(I2C_RECV_BUFFER));
        #[cfg(feature = "dev")]
        {
            resources
                .debug_resources
                .mcu_debug_handler
                .supply_mem_block(MemBlock::new(DEBUG_RECV_BUFFER));
        }
        resources
    }

    #[task(binds = I2C1, resources = [host_interface, state],
        spawn = [erase_spi_flash, write_spi_flash, try_start_fpga, write_spi_flash_test_data,
            test_camera_i2c, signal_interrupt], priority = 3)]
    fn i2c1_interrupt(ctx: i2c1_interrupt::Context) {
        let host_interface = ctx.resources.host_interface;
        match host_interface.handle_interrupt(ctx.resources.state) {
            Some(application::Event::Command(Command::Reset)) => {
                cortex_m::peripheral::SCB::sys_reset()
            }
            Some(application::Event::Command(Command::EraseSpiFlash)) => {
                host_interface.set_spi_flash_erasing(true);
                let _ = ctx.spawn.erase_spi_flash();
            }
            Some(application::Event::WriteSpiFlash(write_request)) => {
                let _ = ctx.spawn.write_spi_flash(write_request);
            }
            Some(application::Event::Command(Command::WriteSpiFlashTestData)) => {
                let _ = ctx.spawn.write_spi_flash_test_data();
            }
            #[cfg(feature = "dev")]
            Some(application::Event::Command(Command::MlbInterrupt)) => {
                let _ = ctx.spawn.signal_interrupt();
            }
            Some(application::Event::Command(Command::LaunchApp)) => {
                // We can safely ignore failed attempts to swap this task
                // because 1 attempt to start the FPGA is equivalent to 2.
                let _ = ctx.spawn.try_start_fpga();
            }
            Some(application::Event::Command(command)) => {
                error!("Unsupported command {:?}", command);
                host_interface.report_error(mcu_common::Error::HOST_I2C_BAD_REQUEST);
                host_interface.command_completed();
            }
            Some(application::Event::TestCameraI2c) => {
                let _ = ctx.spawn.test_camera_i2c();
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

    // Ideally this whole task would be excluded when the dev feature is
    // disabled, however this fails to compile (at least with rtic 0.5.9).
    // Presumably it's trying to include the function in the interrupt table
    // when it doesn't exist.
    #[task(binds = TIM16,
        resources = [debug_resources, fpga_device, timer, camera_i2c, stopwatch,
            debug_led, tick_count, host_interface],
        spawn = [try_start_fpga])]
    fn timer_tick(mut ctx: timer_tick::Context) {
        // Make "use" of ctx to prevent unused variable warning when dev feature
        // is off.
        let _ = &mut ctx;
        #[cfg(feature = "dev")]
        {
            ctx.resources.timer.clear_irq();
            let led = &mut ctx.resources.debug_led;
            *ctx.resources.tick_count = (*ctx.resources.tick_count).wrapping_add(1);
            if *ctx.resources.tick_count & 16 == 0 {
                led.set_low().unwrap();
            } else {
                led.set_high().unwrap();
            }
            while let Some(command) = ctx
                .resources
                .debug_resources
                .mcu_debug_handler
                .get_mcu_debug_command()
            {
                debug::handle_mcu_debug_command(command, &mut ctx.resources, &mut ctx.spawn);
            }
        }
    }

    #[task(resources = [camera_i2c, state, host_interface])]
    fn test_camera_i2c(mut ctx: test_camera_i2c::Context) {
        let mut camera = hm01b0::Camera::new(ctx.resources.camera_i2c);
        loop {
            // For each camera test iteration, we perform 100 reads of the
            // camera ID.
            for _ in 0..100 {
                if !camera
                    .get_camera_id()
                    .map(|id| id == CAMERA_ID)
                    .unwrap_or(false)
                {
                    ctx.resources.host_interface.lock(|host_interface| {
                        host_interface.report_error(mcu_common::Error::CAMERA)
                    });
                    break;
                }
            }
            // Decrement test iteration register and break out of loop if it has
            // reached zero.
            if ctx.resources.state.lock(|state| {
                state.camera_test_iterations = state.camera_test_iterations.saturating_sub(1);
                state.camera_test_iterations == 0
            }) {
                break;
            }
        }
    }

    #[task(resources = [spi, state, debug_led, fpga_device, debug_resources, camera_i2c, host_interface],
        spawn = [handle_spi_packet])]
    fn handle_spi_packet(mut ctx: handle_spi_packet::Context, mut buffer: MemBlock) {
        let request = fpga::Request::from_received_packet(&buffer);
        match &request.action {
            fpga::Action::SetDebugLed => {
                if buffer[1] == 0 {
                    ctx.resources.debug_led.set_low().unwrap();
                } else {
                    ctx.resources.debug_led.set_high().unwrap();
                }
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
            #[cfg(feature = "dev")]
            fpga::Action::DebugLog => {
                if let Some(message) = fpga::payload_as_string(&buffer) {
                    let channel = &mut ctx.resources.debug_resources.fpga_up_channel;
                    use core::fmt::Write;
                    write!(channel, "{}", message).unwrap();
                } else {
                    info!("FPGA sent invalid message");
                }
            }
            #[cfg(feature = "image-transfer")]
            fpga::Action::DebugImage => {
                if !debug::transfer_image(
                    &buffer,
                    &mut ctx.resources.debug_resources.image_channel,
                    ctx.resources.state,
                    ctx.resources.host_interface,
                ) {
                    // Keep repeating this task until the I2C buffer becomes
                    // available and the data can be sent.
                    let _ = ctx.spawn.handle_spi_packet(buffer);
                    return;
                }
            }
            #[cfg(feature = "dev")]
            fpga::Action::GetDebugCommand => {
                ctx.resources
                    .debug_resources
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
            fpga::Action::ReportFpgaError => {
                let error_bits = u16::from_le_bytes([buffer[1], buffer[2]]);
                let error = mcu_common::Error::from_bits_truncate(error_bits);
                info!("FPGA reports error(s): {:?}", error);
                if !error.is_empty() {
                    ctx.resources.host_interface.lock(|h| h.report_error(error));
                }
            }
            fpga::Action::Poll => {}
            fpga::Action::Echo => {}
        }
        request.prepare_to_send(&mut buffer);
        ctx.resources.spi.lock(|spi| spi.send(buffer));
    }

    #[task(resources = [fpga_device, host_interface])]
    fn erase_spi_flash(mut ctx: erase_spi_flash::Context) {
        if let Err(error) = spi_flash::erase(get_spi_flash(ctx.resources.fpga_device).ok()) {
            ctx.resources
                .host_interface
                .lock(|host_interface| host_interface.report_error(error));
        }
        ctx.resources
            .host_interface
            .lock(|host_interface| host_interface.set_spi_flash_erasing(false));
        ctx.resources
            .host_interface
            .lock(|host_interface| host_interface.command_completed());
    }

    #[task(resources = [fpga_device, host_interface])]
    fn write_spi_flash(mut ctx: write_spi_flash::Context, request: i2c_protocol::WriteMemoryEvent) {
        if let Err(error) = spi_flash::write(
            get_spi_flash(ctx.resources.fpga_device).ok(),
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

    #[task(resources = [fpga_device, host_interface, state, rcc])]
    fn try_start_fpga(ctx: try_start_fpga::Context) {
        let mut host_interface = ctx.resources.host_interface;
        match ctx.resources.fpga_device.start_fpga(ctx.resources.rcc) {
            Ok(_) => {
                host_interface.lock(|h| h.set_application_running(true));
            }
            Err(error) => {
                error!("Unable to start FPGA: {:?}", error);
                host_interface.lock(|host_interface| host_interface.report_error(error));
            }
        }
        host_interface.lock(|h| h.command_completed());
    }

    #[task(resources = [fpga_device, host_interface])]
    fn write_spi_flash_test_data(ctx: write_spi_flash_test_data::Context) {
        let mut host_interface = ctx.resources.host_interface;
        if let Err(error) = get_spi_flash(ctx.resources.fpga_device)
            .and_then(|spi_flash| testing::write_spi_test_data(spi_flash))
        {
            host_interface.lock(|h| h.report_error(error));
        }
        host_interface.lock(|h| h.command_completed());
    }

    #[task(resources = [interrupt_pin, host_interface])]
    fn signal_interrupt(mut ctx: signal_interrupt::Context) {
        let _ = ctx.resources.interrupt_pin.set_low();
        // TODO: If we use signal_interrupt for something other than testing and
        // debugging purposes, we should use a timer rather than a delay so that
        // other tasks at this priory can run concurrently. Also, basing the
        // delay on cycles means that changes in our CPU speed will affect the
        // duration. But this is good enough for testing purposes. If the CPU is
        // running at 60MHz, then 500_000 cycles should be about 8ms.
        cortex_m::asm::delay(500_000);
        let _ = ctx.resources.interrupt_pin.set_high();
        ctx.resources.host_interface.lock(|h| h.command_completed());
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
    }
};

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
    let mut init_status = Status::empty();
    match p.FLASH.optr.read().rdp().bits() {
        0xaa => {}
        0xcc => init_status |= Status::STAGE0_PERM_LOCKED,
        _ => init_status |= Status::STAGE0_LOCKED,
    }
    // safety: We're calling this during initialization when interrupts are
    // disabled as required.
    #[allow(unused_variables)]
    let debug_resources = unsafe { debug::DebugResources::init() };

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
    host_interface.init_status = init_status;

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
    );

    let stopwatch = p.TIM2.stopwatch(&mut rcc);

    let mut camera_i2c = board.camera_i2c;

    let pwm = board.camera_mclk_tim.pwm(6.mhz(), &mut rcc);
    let mut pwm_channel = pwm.bind_pin(board.camera_mclk.set_speed(hal::gpio::Speed::VeryHigh));
    pwm_channel.set_duty((pwm_channel.get_max_duty() + 1) / 2);
    pwm_channel.enable();

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
                        host_interface.report_error(mcu_common::Error::SPI_FLASH);
                        info!("Failed to enable quad SPI");
                    }

                    spi_flash = Some(flash);
                } else {
                    host_interface.report_error(mcu_common::Error::SPI_FLASH);
                    info!(
                        "Unexpected flash ID {}, [{}, {}]",
                        id.mfr_code(),
                        id.device_id()[0],
                        id.device_id()[1]
                    );
                }
            }
            Err(_) => {
                host_interface.report_error(mcu_common::Error::SPI_FLASH);
                info!("Failed to read flash ID");
            }
        },
        Err(e) => {
            host_interface.report_error(mcu_common::Error::SPI_FLASH);
            info!("Failed to initialize SPI flash: {:?}", e);
        }
    };

    let mut camera = hm01b0::Camera::new(&mut camera_i2c);
    match camera.get_camera_id() {
        Err(_) => {
            host_interface.report_error(mcu_common::Error::CAMERA);
            info!("Failed to read camera ID");
        }
        Ok(id) => {
            part_ids.camera_id = id as u32;
            if id != CAMERA_ID {
                host_interface.report_error(mcu_common::Error::CAMERA);
                info!("Camera reported unexpected ID: {}", id);
            }
        }
    }
    match camera.run_init_script() {
        Ok(_) => info!("Default HM01B0 configuration applied"),
        Err(_) => info!("Failed to apply HM01B0 init config"),
    };

    info!(
        "MCU application started. CPU is running at {}MHz",
        rcc.clocks.sys_clk.0 / 1000000
    );

    host_interface.part_ids = part_ids;

    let fpga_device = FpgaDevice::new(board.fpga_programn, board.fpga_power_gate, spi_flash);

    // If something tried to address us while we were still initializing, the
    // bus might have ended up in a bad state, reset it.
    host_interface.reset_bus();

    init::LateResources {
        host_interface,
        spi,
        exti,
        state: ApplicationState::default(),
        debug_led: board.debug_led,
        fpga_device,
        #[cfg(feature = "dev")]
        debug_resources,
        #[cfg(feature = "dev")]
        timer: debug::init_debug_timer(p.TIM16, &mut rcc),
        camera_i2c,
        stopwatch,
        tick_count: 0,
        rcc,
        interrupt_pin: board.interrupt_pin,
    }
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
