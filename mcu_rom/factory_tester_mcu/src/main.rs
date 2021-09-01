// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

mod gateware_writer;

use core::convert::Infallible;
use cortex_m::delay::Delay;
use cortex_m_rt::entry;
use hal::gpio::gpioa::PA11;
use hal::gpio::gpioa::PA12;
use hal::gpio::gpioa::PA5;
use hal::gpio::gpioa::PA6;
use hal::gpio::gpioa::PA7;
use hal::gpio::gpiob::PB0;
use hal::gpio::Analog;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::i2c::I2c;
use hal::prelude::*;
use hal::spi::Spi;
use hal::stm32;
use hal::stm32::I2C2;
use hal::timer::stopwatch::Stopwatch;
use spi_memory::series25::Flash;
use stm32g0 as _;
use stm32g0::stm32g071::SPI1;
use stm32g0::stm32g071::TIM2;

#[cfg(feature = "dev")]
use panic_rtt_target as _;

#[cfg(feature = "dev")]
use rtt_target::rprintln;

#[cfg(not(feature = "dev"))]
use panic_halt as _;

// If we're in non-dev mode, then all uses of rprintln dissapear. We use
// format_args so that we don't get warnings about arguments to rprintln being
// unused.
#[cfg(not(feature = "dev"))]
#[macro_export]
macro_rules! rprintln {
    ($($x:tt)*) => {
        format_args!($($x)*);
    };
}

type FlashSpi = Spi<SPI1, (PA5<Analog>, PA6<Analog>, PA7<Analog>)>;
type FlashSpiCs = PB0<Output<OpenDrain>>;
type CameraI2c = I2c<I2C2, PA12<Output<OpenDrain>>, PA11<Output<OpenDrain>>>;

pub struct Tester {
    camera_i2c: CameraI2c,
    flash_spi: Option<FlashSpi>,
    flash_spi_cs: Option<FlashSpiCs>,
    stopwatch: Stopwatch<TIM2>,
}

#[derive(Debug)]
enum Error {
    I2cError,
    /// Reported if there is a bug in the test software.
    InternalError,
    SpiFlashError,
    UnexpectedSpiFlashId,
}

#[entry]
fn main() -> ! {
    #[cfg(feature = "dev")]
    {
        rtt_target::rtt_init_print!();
    }

    let p = stm32::Peripherals::take().unwrap();
    let cp = cortex_m::Peripherals::take().unwrap();

    // Reset system clock source to the HSI. The factory bootloader (which we
    // are run from without a CPU reset), will have set the clock source to the
    // PLL. If we try to configure the PLL without first switching back to the
    // HSI, then we'll go into an infinite loop while we wait for the PLL to
    // disable, which it will never do, since it's being used.
    p.RCC.cfgr.write(|w| w);
    let mut rcc = p
        .RCC
        .freeze(hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default()));

    rprintln!("HPS tester starting");

    let gpioa = p.GPIOA.split(&mut rcc);
    let gpiob = p.GPIOB.split(&mut rcc);
    let gpioc = p.GPIOC.split(&mut rcc);

    let mut led = gpioa.pa1.into_push_pull_output();
    let stopwatch = p.TIM2.stopwatch(&mut rcc);
    let mut delay = Delay::new(cp.SYST, rcc.clocks.ahb_clk.0);

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

    let mut tester = Tester {
        camera_i2c,
        flash_spi,
        flash_spi_cs,
        stopwatch,
    };
    match tester.run_test() {
        Ok(_) => {
            rprintln!("Test successful");
            led.set_high().unwrap();
        }
        Err(error) => {
            rprintln!("Test failed: {:?}", error);
        }
    }

    // Force reentry into system bootloader, but only in non-dev mode, since it
    // would be annoying to have that happen if dev mode.
    #[cfg(not(feature = "dev"))]
    {
        p.SYSCFG_VREFBUF
            .cfgr1
            .modify(|_, w| unsafe { w.mem_mode().bits(1) });
        cortex_m::peripheral::SCB::sys_reset();
    }
    #[cfg(feature = "dev")]
    loop {}
}

impl Tester {
    fn run_test(&mut self) -> Result<(), Error> {
        if let (Some(flash_spi), Some(flash_spi_cs)) =
            (self.flash_spi.take(), self.flash_spi_cs.take())
        {
            let mut flash = Flash::init(flash_spi, flash_spi_cs).map_err(|error| {
                rprintln!("Failed to initialize SPI flash: {:?}", error);
                Error::SpiFlashError
            })?;
            let start_time = self.stopwatch.now();
            gateware_writer::write_gateware(&mut flash)?;
            let elapsed = self.stopwatch.elapsed(start_time);
            rprintln!("Wrote gateware to SPI flash in {}ms", elapsed.0 / 1000);
            let start_time = self.stopwatch.now();
            gateware_writer::verify_spi_flash(&mut flash)?;
            let elapsed = self.stopwatch.elapsed(start_time);
            rprintln!("Verified gateware in {}ms", elapsed.0 / 1000);
        } else {
            rprintln!("run_test called multiple times?");
            return Err(Error::InternalError);
        }

        self.configure_camera()
    }

    fn configure_camera(&mut self) -> Result<(), Error> {
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
        camera.set_frame_length(0x172)?;
        camera.set_test_pattern(Some(hm01b0::TestPattern::Walking1s))?;
        camera.set_mode(hm01b0::Mode::Streaming)?;
        Ok(())
    }
}

impl From<hm01b0::Error> for Error {
    fn from(e: hm01b0::Error) -> Self {
        match e {
            hm01b0::Error::I2cError => Error::I2cError,
        }
    }
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        Error::InternalError
    }
}
