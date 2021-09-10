// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

mod gateware_writer;
mod image_receiver;

use cortex_m::asm;
use factory_tester_common::Error;
use factory_tester_common::TEST_CONTROLLER_I2C_ADDRESS;
use hal::gpio::gpioa::PA1;
use hal::gpio::gpioa::PA11;
use hal::gpio::gpioa::PA12;
use hal::gpio::gpioa::PA3;
use hal::gpio::gpioa::PA5;
use hal::gpio::gpioa::PA6;
use hal::gpio::gpioa::PA7;
use hal::gpio::gpiob::PB0;
use hal::gpio::gpiob::PB6;
use hal::gpio::gpiob::PB7;
use hal::gpio::gpioc::PC15;
use hal::gpio::Analog;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::gpio::PullUp;
use hal::gpio::PushPull;
use hal::i2c::I2c;
use hal::prelude::*;
use hal::rcc::Rcc;
use hal::spi::Spi;
use hal::stm32::I2C1;
use hal::stm32::I2C2;
use hal::stm32::SPI1;
use hal::stm32::SYSCFG_VREFBUF;
use hal::stm32::TIM2;
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
type HostI2c = I2c<I2C1, PB7<Output<OpenDrain>>, PB6<Output<OpenDrain>>>;

// I2C bus speed = 400KHz. This value was computed by STM32CubeMX with APB clock
// at 64MHz, rise time of 190ns and a fall time of 20ns.
const I2C_TIMING_BITS: u32 = 0x10901032;

const CPU_MHZ: u32 = 64;
const CYCLES_PER_US: u32 = CPU_MHZ;
const CYCLES_PER_MS: u32 = CYCLES_PER_US * 1000;

#[cfg(feature = "dev")]
type Stopwatch = hal::timer::stopwatch::Stopwatch<TIM2>;

#[cfg(not(feature = "dev"))]
type Stopwatch = NullStopwatch;

pub struct Tester {
    camera_i2c: CameraI2c,
    flash_spi: Option<FlashSpi>,
    flash_spi_cs: Option<FlashSpiCs>,
    fpga_programn: PC15<Output<PushPull>>,
    stopwatch: Stopwatch,

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
        host_i2c: HostI2c,
    }

    #[init(spawn = [run_test])]
    fn init(ctx: init::Context) -> init::LateResources {
        ctx.spawn.run_test().unwrap();
        init_peripherals(ctx.device, ctx.core)
    }

    #[task(resources = [tester, syscfg_vrefbuf, debug_led, host_i2c])]
    fn run_test(ctx: run_test::Context) {
        match ctx.resources.tester.run_test() {
            Ok(_) => {
                if cfg!(not(feature = "dev")) {
                    report_result(ctx.resources.host_i2c, &[0]);
                }
                info!("Test successful");
            }
            Err(error) => {
                if cfg!(not(feature = "dev")) {
                    report_result(ctx.resources.host_i2c, &[error as u8]);
                }
                error!("Test failed: {:?}", error);
            }
        }
        if cfg!(feature = "dev") {
            // In dev mode, we wait 100ms before we reset. This is to give the
            // RTT channel a chance to be read, otherwise the test status won't
            // be visible. This reset is useful in dev mode as it results in the
            // test repeating.
            asm::delay(100 * CYCLES_PER_MS);
        } else {
            // In non-dev mode, need make sure I2C has a chance to finish
            // sending the test result back to the controller. We want to wait a
            // short time here, since the test controller needs to write the
            // test log using the system bootloader before the test finishes, so
            // we're still on-the-clock. With I2C running at 400KHz, a byte plus
            // ack and stop bits takes 25us, so we allow 4x that.
            asm::delay(100 * CYCLES_PER_US);
        }
        cortex_m::peripheral::SCB::sys_reset();
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
    // it ourselves. Also, it seems that if we put our vector table at
    // 0x20002700, it doesn't work, whereas 0x20002800 does work. This is odd,
    // since only the bottom 7 bits of the vtor register are reserved, but it
    // seems to behave as though it ignores the bottom 9. If we went and put our
    // vector table at 0x20002800, it would work, but we'd have 256 bytes less
    // to store our program. So instead, we copy our vector table to the start
    // of RAM. Our linker script makes sure the first 1KB of RAM is kept free.
    const VECTOR_TABLE_DEST: usize = 0x20000000;
    unsafe {
        core::ptr::copy_nonoverlapping(&VECTOR_TABLE as *const _, VECTOR_TABLE_DEST as *mut _, 256)
    };
    unsafe { cp.SCB.vtor.write(VECTOR_TABLE_DEST as u32) };
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

    let stopwatch = create_stopwatch(p.TIM2, &mut rcc);
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

    let sda = gpiob.pb7.into_floating_input();
    let scl = gpiob.pb6.into_floating_input();
    let host_i2c = p.I2C1.i2c(
        sda.into_open_drain_output(),
        scl.into_open_drain_output(),
        hal::i2c::Config::with_timing(I2C_TIMING_BITS),
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
    asm::delay(5 * CYCLES_PER_MS);

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
        host_i2c,
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

fn report_result(i2c: &mut HostI2c, result: &[u8]) {
    use embedded_hal::blocking::i2c::Write;
    // If we failed really quickly, the test controller might not have had a
    // chance to reconfigure its I2C to receive our response, so we give it a
    // few tries, with a delay between each.
    for _ in 0..10 {
        if i2c.write(TEST_CONTROLLER_I2C_ADDRESS, result).is_ok() {
            return;
        }
        asm::delay(10 * CYCLES_PER_MS);
    }
    error!("Failed to send data back to tester");
}

/// A null implementation of a stopwatch. This is used in non-dev mode when we
/// don't measure times in order to save space in the compiled binary.

#[cfg(not(feature = "dev"))]
struct NullStopwatch;

#[cfg(not(feature = "dev"))]
impl NullStopwatch {
    fn now(&self) -> u32 {
        0
    }

    fn elapsed(&self, _: u32) -> hal::time::MicroSecond {
        hal::time::MicroSecond(0)
    }
}

#[cfg(feature = "dev")]
fn create_stopwatch(tim2: TIM2, rcc: &mut Rcc) -> Stopwatch {
    tim2.stopwatch(rcc)
}

#[cfg(not(feature = "dev"))]
fn create_stopwatch(_tim2: TIM2, _rcc: &mut Rcc) -> Stopwatch {
    NullStopwatch
}
