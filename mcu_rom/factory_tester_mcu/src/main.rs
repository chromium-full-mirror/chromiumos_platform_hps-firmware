// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

mod gateware_writer;
mod image_receiver;

use cortex_m::delay::Delay;
use factory_tester_common::Error;
use hal::gpio::gpioa::PA1;
use hal::gpio::gpioa::PA11;
use hal::gpio::gpioa::PA12;
use hal::gpio::gpioa::PA3;
use hal::gpio::gpioa::PA5;
use hal::gpio::gpioa::PA6;
use hal::gpio::gpioa::PA7;
use hal::gpio::gpiob::PB0;
use hal::gpio::gpioc::PC15;
use hal::gpio::Analog;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::gpio::PullUp;
use hal::gpio::PushPull;
use hal::i2c::I2c;
use hal::prelude::*;
use hal::spi::Spi;
use hal::stm32::I2C2;
use hal::stm32::SPI1;
use hal::stm32::SYSCFG_VREFBUF;
use hal::stm32::TIM2;
use hal::timer::stopwatch::Stopwatch;
use log::error;
use log::info;
use spi_memory::series25::Flash;
use stm32g0 as _;

#[cfg(feature = "dev")]
use panic_rtt_target as _;

#[cfg(not(feature = "dev"))]
use panic_halt as _;

extern "C" {
    pub static VECTOR_TABLE: u32;
}

type FlashSpi = Spi<SPI1, (PA5<Analog>, PA6<Analog>, PA7<Analog>)>;
type FlashSpiCs = PB0<Output<OpenDrain>>;
type CameraI2c = I2c<I2C2, PA12<Output<OpenDrain>>, PA11<Output<OpenDrain>>>;

pub struct Tester {
    camera_i2c: CameraI2c,
    flash_spi: Option<FlashSpi>,
    flash_spi_cs: Option<FlashSpiCs>,
    fpga_programn: PC15<Output<PushPull>>,
    stopwatch: Stopwatch<TIM2>,

    // Data sources from the FPGA
    heartbeat: PA3<Input<PullUp>>,
    fpga_spi: hal::stm32::SPI2,
    fpga_spi_sck: hal::gpio::gpioa::PA0<Analog>,
    fpga_spi_copi: hal::gpio::gpioa::PA4<Analog>,
}

#[rtic::app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        tester: Tester,
        syscfg_vrefbuf: SYSCFG_VREFBUF,
        debug_led: PA1<Output<PushPull>>,
    }

    #[init(spawn = [run_test])]
    fn init(ctx: init::Context) -> init::LateResources {
        ctx.spawn.run_test().unwrap();
        init_peripherals(ctx.device, ctx.core)
    }

    #[task(resources = [tester, syscfg_vrefbuf, debug_led])]
    fn run_test(ctx: run_test::Context) {
        match ctx.resources.tester.run_test() {
            Ok(_) => {
                info!("Test successful");
            }
            Err(error) => {
                error!("Test failed: {:?}", error);
            }
        }
    }

    // Interrupts that we're not using that can be used by RTIC for spawning
    // software tasks.
    extern "C" {
        fn TIM17();
    }
};

fn init_peripherals(
    p: hal::stm32::Peripherals,
    cp: rtic::export::Peripherals,
) -> init::LateResources {
    // SAFETY: Must be called during initialization while interrupts are
    // disabled.
    unsafe { debug_logger::initialize_logging() };

    // Enable, then reset SPI2.
    p.RCC.apbenr1.write(|w| w.spi2en().set_bit());
    p.RCC.apbrstr1.write(|w| w.spi2rst().set_bit());
    p.RCC.apbrstr1.write(|w| w.spi2rst().clear_bit());

    // Reset system clock source to the HSI. The factory bootloader (which we
    // are run from without a CPU reset), will have set the clock source to the
    // PLL. If we try to configure the PLL without first switching back to the
    // HSI, then we'll go into an infinite loop while we wait for the PLL to
    // disable, which it will never do, since it's being used.
    p.RCC.cfgr.write(|w| w);
    // The system bootloader doesn't configure the vector table for us, so we do
    // it ourselves.
    unsafe { cp.SCB.vtor.write(&VECTOR_TABLE as *const _ as u32) };
    // Enable DMA clock so that RTT (the mechanism used to send defmt messages)
    // is still handled when we're idle and the CPU is put to sleep.
    p.RCC.ahbenr.modify(|_, w| w.dmaen().set_bit());
    let mut rcc = p
        .RCC
        .freeze(hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default()));

    info!("HPS tester starting");

    let gpioa = p.GPIOA.split(&mut rcc);
    let gpiob = p.GPIOB.split(&mut rcc);
    let gpioc = p.GPIOC.split(&mut rcc);

    let stopwatch = p.TIM2.stopwatch(&mut rcc);
    let mut delay = Delay::new(cp.SYST, rcc.clocks.ahb_clk.0);
    let mut debug_led = gpioa.pa1.into_push_pull_output();
    debug_led.set_high().unwrap();

    let heartbeat = gpioa.pa3.into_pull_up_input();

    // Set the MLB interrupt line low. It has a pull-up resistor, so should have
    // been high before. The test controller can then check that it's high when
    // it starts and that it then goes low.
    let mut mlb_int = gpiob.pb8.into_open_drain_output();
    mlb_int.set_low().unwrap();

    // FW_WP has no pull-up resistor. We set it high now then low later on. The
    // test controller can count the transitions.
    let mut fwwp = gpioa.pa15.into_push_pull_output();
    fwwp.set_high().unwrap();

    // Enable a clock for the camera.
    let pwm = p.TIM3.pwm(6.mhz(), &mut rcc);
    let mut pwm_channel = pwm.bind_pin(gpiob.pb1.set_speed(hal::gpio::Speed::VeryHigh));
    pwm_channel.set_duty((pwm_channel.get_max_duty() + 1) / 2);
    pwm_channel.enable();

    let camera_i2c = p.I2C2.i2c(
        gpioa.pa12.into_open_drain_output(), // SDA
        gpioa.pa11.into_open_drain_output(), // SCL
        hal::i2c::Config::new(400.khz()),
        &mut rcc,
    );

    // Put FPGA into reset before we power it on to ensure that it doesn't try
    // to access the SPI flash at the same time as us.
    let mut fpga_programn = gpioc.pc15.into_push_pull_output();
    fpga_programn.set_low().unwrap();

    // Ensure FPGA and SPI flash are powered.
    let mut fpga_power_gate = gpiob.pb5.into_push_pull_output();
    fpga_power_gate.set_low().unwrap();

    // Give the SPI flash chip a chance to start after we powered it on. The
    // datasheet says minimum time is 2.5ms. To be generous, we give it double
    // that.
    delay.delay_ms(5u32);

    fwwp.set_low().unwrap();

    let flash_spi_cs = Some(gpiob.pb0.into_open_drain_output());
    let flash_spi_clk = gpioa.pa5;
    let flash_spi_copi = gpioa.pa7;
    let flash_spi_cipo = gpioa.pa6;
    let flash_spi = Some(p.SPI1.spi(
        (flash_spi_clk, flash_spi_cipo, flash_spi_copi),
        hal::spi::MODE_0,
        4.mhz(),
        &mut rcc,
    ));

    let tester = Tester {
        camera_i2c,
        flash_spi,
        flash_spi_cs,
        fpga_programn,
        stopwatch,
        heartbeat,
        fpga_spi: p.SPI2,
        fpga_spi_sck: gpioa.pa0,
        fpga_spi_copi: gpioa.pa4,
    };

    init::LateResources {
        tester,
        syscfg_vrefbuf: p.SYSCFG_VREFBUF,
        debug_led,
    }
}

impl Tester {
    fn run_test(&mut self) -> Result<(), Error> {
        if let (Some(flash_spi), Some(flash_spi_cs)) =
            (self.flash_spi.take(), self.flash_spi_cs.take())
        {
            let mut flash = Flash::init(flash_spi, flash_spi_cs).map_err(|error| {
                error!("Failed to initialize SPI flash: {:?}", error);
                Error::SpiFlashCommunication
            })?;
            gateware_writer::write_gateware(&mut flash, &mut self.stopwatch)?;
        } else {
            error!("run_test called multiple times?");
            return Err(Error::Internal);
        }

        // Start FPGA
        self.fpga_programn.set_high()?;

        check_heartbeat(&self.heartbeat)?;

        self.configure_camera().map_err(convert_camera_error)?;

        image_receiver::validate_image_data(
            &mut self.fpga_spi,
            &mut self.fpga_spi_sck,
            &mut self.fpga_spi_copi,
        )
    }

    fn configure_camera(&mut self) -> Result<(), hm01b0::Error> {
        let mut camera = hm01b0::Camera::new(&mut self.camera_i2c);
        // Sometimes the first i2c access to the camera fails (camera doesn't
        // ACK), so we do a few reads until it succeeds.
        for _ in 0..10 {
            if camera.get_camera_id().is_ok() {
                break;
            }
        }
        camera.reset()?;
        camera.set_advance_vsync(1)?;
        camera.set_shift_config(hm01b0::ShiftConfig {
            pixel_shift_enabled: false,
            hsync_shift_enabled: false,
            vsync_shift_enabled: true,
        })?;
        camera.set_qvga_enable(true)?;
        camera.set_line_length(0x0178)?;
        camera.set_frame_length(0x178)?;
        camera.set_test_pattern(Some(hm01b0::TestPattern::ColorBar))?;
        camera.set_mode(hm01b0::Mode::Streaming)?;
        Ok(())
    }
}

/// Waits until the pin `heartbeat` is observed both high and low.
fn check_heartbeat(heartbeat: &PA3<Input<PullUp>>) -> Result<(), Error> {
    let start_state = heartbeat.is_high()?;
    // Generally it takes about 2.94M iterations before we get a signal, so we
    // double that to 6M.
    for x in 0..6_000_000 {
        if heartbeat.is_high()? != start_state {
            info!("Got signal from FPGA after {} iterations", x);
            return Ok(());
        }
    }
    Err(Error::FpgaNotStarted)
}

fn convert_camera_error(e: hm01b0::Error) -> Error {
    match e {
        hm01b0::Error::I2cError => Error::CameraI2c,
    }
}
