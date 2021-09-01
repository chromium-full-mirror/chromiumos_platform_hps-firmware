// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use hal::gpio::Edge;
use hal::gpio::Floating;
use panic_probe as _;

use factory_tester_common::Error;
use hal::gpio::Input;
use hal::gpio::PC13;
use hal::prelude::*;
use hal::rcc::PllConfig;
use hal::rcc::PllDivider;
use hal::rtc::Rtc;
use hal::rtc::RtcClockSource;
use hal::rtc::RtcConfig;
use hal::stm32;
use log::error;
use log::info;

type Result<T, E = Error> = core::result::Result<T, E>;

pub struct Tester {
    rtc: Rtc,
}

#[rtic::app(device = hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        tester: Tester,
        button: PC13<Input<Floating>>,
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

    #[task(binds = EXTI15_10, resources = [tester, button])]
    fn exti15_10(ctx: exti15_10::Context) {
        let tester = ctx.resources.tester;
        let start_ms = tester.get_millis();
        match tester.run_test() {
            Ok(_) => {
                let end_ms = tester.get_millis();
                info!("Test passed in {}ms", end_ms - start_ms);
            }
            Err(error) => error!("Test failed: {:?}", error),
        }
        ctx.resources.button.clear_interrupt_pending_bit();
    }
};

fn init_peripherals(p: stm32::Peripherals) -> init::LateResources {
    initialize_logging();
    let mut flash = p.FLASH.constrain();
    let mut rcc = p.RCC.constrain();
    let mut pwr = p.PWR.constrain(&mut rcc.apb1r1);
    rcc
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

    let tester = Tester { rtc };

    init::LateResources { tester, button }
}

impl Tester {
    fn run_test(&mut self) -> Result<()> {
        info!("Starting test");

        // TODO: Test some stuff.

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
