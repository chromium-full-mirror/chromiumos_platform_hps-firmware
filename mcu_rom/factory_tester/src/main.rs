// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use hal::gpio::Edge;
use hal::gpio::Floating;
use hal::gpio::PC3;
use panic_probe as _;

use delayed_i2c::DelayedI2c;
use factory_tester_common::Error;
use hal::delay::DelayCM;
use hal::device::I2C3;
use hal::gpio::Alternate;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::gpio::PushPull;
use hal::gpio::AF4;
use hal::gpio::PB13;
use hal::gpio::PC0;
use hal::gpio::PC1;
use hal::gpio::PC13;
use hal::i2c::I2c;
use hal::prelude::*;
use hal::rcc::PllConfig;
use hal::rcc::PllDivider;
use hal::rtc::Rtc;
use hal::rtc::RtcClockSource;
use hal::rtc::RtcConfig;
use hal::stm32;
use log::error;
use log::info;
use mcu_common::HPS_ADDRESS;
use stm32_bootloader_client::Stm32;

type Result<T, E = Error> = core::result::Result<T, E>;

const MCU_CODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu.bin"));
/// The address at which we load our MCU code. This needs to be after the memory
/// used by the factory bootloader. This address must be the same as what get
/// written to memory.x by build.rs for the factory_test_mcu crate.
const MCU_LOAD_ADDRESS: u32 = 0x20002700;

/// The ID of the MCU used in the HPS. STM32G07xxx/08xxx - see AN2606 rev 49,
/// table 152.
const EXPECTED_CHIP_ID: u16 = 0x460;

/// See AN2606 for the I2C address for each chip. 0x51 is STM32G07xxx/08xxx.
const FACTORY_BOOTLOADER_I2C_ADDRESS: u8 = 0x51;

// I2C timing configuration bits. This value was computed by STM32CubeMX with
// mode set to "Fast Mode" and frequency set to 400KHz and PCLK at 20MHz. Using
// fast mode plus (e.g 1MHz) works when two STM32s are linked directly, but
// doesn't work with proto2, presumably because fast mode plus is driven and the
// I2C is going via isolation FETs that assume open drain.
const I2C_TIMING_BITS: u32 = 0x0050081E;

// How long we'll wait for the MCU to send us a signal on the interrupt line
// after we tell it to start. This timeout is very generous, since the actual
// time is <1ms.
const MCU_START_TIMEOUT_MS: u32 = 100;

type I2cToDut = I2c<
    I2C3,
    (
        PC0<Alternate<AF4, Output<OpenDrain>>>,
        PC1<Alternate<AF4, Output<OpenDrain>>>,
    ),
>;

pub struct Tester {
    rtc: Rtc,
    i2c: DelayedI2c<I2cToDut, DelayCM>,
    delay: DelayCM,
    mlb_int: PC3<Input<Floating>>,
}

#[rtic::app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        tester: Tester,
        button: PC13<Input<Floating>>,
        led: PB13<Output<PushPull>>,
    }

    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        init_peripherals(ctx.device)
    }

    // TOOD: Figure out how to make RTT work without this (we did this for the
    // stm32g0, but need to find the equivalent for the stm32l4).
    #[idle(resources = [])]
    fn idle(_ctx: idle::Context) -> ! {
        info!("Press button to start test");
        loop {}
    }

    #[task(binds = EXTI15_10, resources = [tester, button, led])]
    fn exti15_10(ctx: exti15_10::Context) {
        let tester = ctx.resources.tester;
        let start_ms = tester.get_millis();
        let led = ctx.resources.led;
        led.set_low().unwrap();
        match tester.run_test() {
            Ok(_) => {
                let end_ms = tester.get_millis();
                // Set the LED to be on to indicate that the test passed.
                led.set_high().unwrap();
                info!("Test passed in {}ms", end_ms - start_ms);
            }
            Err(error) => {
                error!("Test failed: {:?}", error);
                // Blink the LED N times to indicate failure code N. We repeat
                // this 3 times to give the user a chance to count the blinks.
                // Longer term, we probably want an easier way to read the test
                // status - e.g. a small numeric display.
                for _ in 0..3 {
                    for _ in 0..error.into() {
                        led.set_high().unwrap();
                        tester.delay.delay_ms(150u32);
                        led.set_low().unwrap();
                        tester.delay.delay_ms(150u32);
                    }
                    tester.delay.delay_ms(1200u32);
                }
            }
        }
        ctx.resources.button.clear_interrupt_pending_bit();
    }
};

fn init_peripherals(p: stm32::Peripherals) -> init::LateResources {
    initialize_logging();
    let mut flash = p.FLASH.constrain();
    let mut rcc = p.RCC.constrain();
    let mut pwr = p.PWR.constrain(&mut rcc.apb1r1);
    let clocks = rcc
        .cfgr
        .sysclk_with_pll(40.mhz(), PllConfig::new(1, 10, PllDivider::Div4))
        .pclk1(20.mhz())
        .freeze(&mut flash.acr, &mut pwr);

    let rtc = Rtc::rtc(
        p.RTC,
        &mut rcc.apb1r1,
        &mut rcc.bdcr,
        &mut pwr.cr1,
        RtcConfig::default().clock_config(RtcClockSource::LSI),
    );

    let mut gpiob = p.GPIOB.split(&mut rcc.ahb2);
    let mut gpioc = p.GPIOC.split(&mut rcc.ahb2);
    let mut exti = p.EXTI;
    let mut syscfg = p.SYSCFG;

    let mut led = gpiob
        .pb13
        .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);
    led.set_high().unwrap();

    // Trigger an interrupt when the button is pressed.
    let mut button = gpioc
        .pc13
        .into_floating_input(&mut gpioc.moder, &mut gpioc.pupdr);
    button.make_interrupt_source(&mut syscfg, &mut rcc.apb2);
    button.enable_interrupt(&mut exti);
    button.trigger_on_edge(&mut exti, Edge::Falling);

    // Monitor MLB_INT pin. This pin has a pull-up resistor, so should initially
    // be high. The MCU test firmware once it starts should pull it low.
    let mlb_int = gpioc
        .pc3
        .into_floating_input(&mut gpioc.moder, &mut gpioc.pupdr);

    let sda = gpioc.pc1;
    let scl = gpioc.pc0;

    let mut sda = sda.into_open_drain_output(&mut gpioc.moder, &mut gpioc.otyper);
    let mut scl = scl.into_open_drain_output(&mut gpioc.moder, &mut gpioc.otyper);
    sda.internal_pull_up(&mut gpioc.pupdr, false);
    scl.internal_pull_up(&mut gpioc.pupdr, false);
    let sda = sda.into_af4(&mut gpioc.moder, &mut gpioc.afrl);
    let scl = scl.into_af4(&mut gpioc.moder, &mut gpioc.afrl);

    let i2c_config = hal::i2c::Config::with_timing(I2C_TIMING_BITS);
    let i2c = I2c::i2c3(p.I2C3, (scl, sda), i2c_config, &mut rcc.apb1r1);
    let delayed_i2c = DelayedI2c {
        i2c,
        delay: DelayCM::new(clocks),
        duration_us: 2,
    };

    let tester = Tester {
        rtc,
        i2c: delayed_i2c,
        delay: DelayCM::new(clocks),
        mlb_int,
    };

    init::LateResources {
        tester,
        button,
        led,
    }
}

impl Tester {
    fn run_test(&mut self) -> Result<()> {
        info!("Starting test");

        if self.mlb_int.is_low().unwrap_or_default() {
            error!("MLB_INT line is low at start of test, when it is supposed to be pulled high");
            return Err(Error::InterruptLine);
        }

        // Check to see if we're connected to a HPS that already has firmware
        // loaded. For now, we don't support testing devices that already have
        // firmware loaded.
        // TODO: Support testing such devices that already have stage0 loaded.
        let mut response = [0u8; 2];
        if self
            .i2c
            .write_read(HPS_ADDRESS, &[0x80], &mut response)
            .is_ok()
            && response == [0x9d, 0xf2]
        {
            error!("Detected HPS that already has firmware loaded.");
            return Err(Error::FirmwareAlreadyLoaded);
        }

        let stm32_config =
            stm32_bootloader_client::Config::i2c_address(FACTORY_BOOTLOADER_I2C_ADDRESS);
        let mut stm32 = Stm32::new(&mut self.i2c, &mut self.delay, stm32_config);

        match stm32.get_chip_id() {
            Ok(EXPECTED_CHIP_ID) => {
                // Nothing to do.
            }
            Ok(id) => {
                error!("Unexpected chip ID: {}", id);
                return Err(Error::UnexpectedMcuId);
            }
            Err(error) => {
                error!("Failed to read chip ID: {:?}", &error);
                return Err(Error::SystemBootloader);
            }
        }

        stm32
            .write_bulk(MCU_LOAD_ADDRESS, MCU_CODE, |_| {})
            .map_err(|error| {
                error!("Failed to write MCU memory: {}", &error);
                Error::SystemBootloader
            })?;

        stm32.go(MCU_LOAD_ADDRESS).map_err(|error| {
            error!("Failed to start MCU program: {}", &error);
            Error::SystemBootloader
        })?;

        let mcu_start_ms = self.get_millis();
        while self.mlb_int.is_high().unwrap_or_default() {
            if self.get_millis() - mcu_start_ms > MCU_START_TIMEOUT_MS {
                error!(
                    "MCU failed to pull MLB_INT low after {}ms",
                    MCU_START_TIMEOUT_MS
                );
                return Err(Error::InterruptLine);
            }
        }

        // Wait for MCU to indicate that it has finished.
        // TODO: Add a timeout.
        while self.mlb_int.is_low().unwrap_or_default() {}

        Ok(())
    }

    /// Returns the current time as milliseconds. Provided the the time hasn't been
    /// set, this will be the time since reset. Will wrap after 31 days.
    fn get_millis(&self) -> u32 {
        // Our HAL crate doesn't yet have a stopwatch feature, so we use the RTC
        // instead.
        let (date, time) = self.rtc.get_date_time();
        time.micros / 1000
            + (((date.day * 24 + time.hours) * 60 + time.minutes) * 60 + time.seconds) * 1000
    }
}

struct RttLogger;
static LOGGER: RttLogger = RttLogger;

pub fn initialize_logging() {
    rtt_target::rtt_init_print!();
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}

impl log::Log for RttLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::LevelFilter::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            rtt_target::rprintln!("{}: {}", record.metadata().level(), record.args());
        }
    }

    fn flush(&self) {}
}
