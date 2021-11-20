// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use core::convert::TryFrom;
use core::ops::Range;
use delayed_i2c::DelayedI2c;
use factory_tester_common::Error;
use factory_tester_common::TestStatus;
use factory_tester_common::ALLOWED_SPI_FLASH_IDS;
use factory_tester_common::EXPECTED_CHIP_ID;
use factory_tester_common::FACTORY_BOOTLOADER_I2C_ADDRESS;
use factory_tester_common::MCU_LOAD_ADDRESS;
use factory_tester_common::MCU_TESTER_I2C_ADDRESS;
use factory_tester_common::TEST_STATUS_SIZE;
use hal::delay::DelayCM;
use hal::device::I2C3;
use hal::gpio::Alternate;
use hal::gpio::Edge;
use hal::gpio::Floating;
use hal::gpio::Input;
use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::gpio::PullUp;
use hal::gpio::PushPull;
use hal::gpio::AF4;
use hal::gpio::PB13;
use hal::gpio::PC0;
use hal::gpio::PC1;
use hal::gpio::PC13;
use hal::gpio::PC2;
use hal::gpio::PC3;
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
use log::warn;
use mcu_common::HPS_ADDRESS;
use panic_probe as _;
use stm32_bootloader_client::Stm32;

type Result<T, E = Error> = core::result::Result<T, E>;

const MCU_CODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/factory_tester_mcu.bin"));

// I2C timing configuration bits. This value was computed by STM32CubeMX with
// mode set to "Fast Mode" and frequency set to 400KHz and PCLK at 20MHz. Using
// fast mode plus (e.g 1MHz) works when two STM32s are linked directly, but
// doesn't work with proto2, presumably because fast mode plus is driven and the
// I2C is going via isolation FETs that assume open drain.
const I2C_TIMING_BITS: u32 = 0x0050081E;

// How long we'll wait for the MCU to send us a signal on the interrupt and
// FW_WP lines after we tell it to start. The interrupt line signal is generally
// <1ms. The FW_WP signal isn't sent until after various things are initialized,
// so ends up around the 7 to 12 ms mark. Still well short of this timeout.
const MCU_START_TIMEOUT_MS: u32 = 100;

/// Range of addresses in the MCU at which we log test results. This is the
/// second half of the flash (64KB-128KB)
const LOG_AREA: Range<u32> = 0x08010000..0x08020000;

/// An arbitrary version number that we write to the test log stored on the
/// MCU's flash.
const TESTER_VERSION: u16 = 1;

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
    fw_wp: PC2<Input<PullUp>>,
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

    #[task(binds = EXTI15_10, resources = [tester, button, led])]
    fn exti15_10(ctx: exti15_10::Context) {
        let tester = ctx.resources.tester;
        let start_ms = tester.get_millis();
        let led = ctx.resources.led;
        led.set_low().unwrap();
        let mut test_result = tester.run_test();
        if let Err(error) = tester.log_result(test_result) {
            test_result = Err(error);
        }
        match test_result {
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
    // Make sure that SWD is still available when RTIC puts the CPU to sleep
    // with the WFI (wait for interrupt) instruction.
    p.DBGMCU.cr.modify(|_, w| {
        w.dbg_sleep().set_bit();
        w.dbg_standby().set_bit();
        w.dbg_stop().set_bit()
    });
    p.RCC.ahb1enr.modify(|_, w| w.dma1en().set_bit());

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

    // Monitor FW_WP pin. This pin has no external pull-up resistor, so we pull it up.
    let fw_wp = gpioc
        .pc2
        .into_pull_up_input(&mut gpioc.moder, &mut gpioc.pupdr);

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
        fw_wp,
    };

    info!("Press button to start test");

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

        if self.fw_wp.is_low().unwrap_or_default() {
            error!("FW_WP line is low at start of test, when it is supposed to be pulled high");
            return Err(Error::FwwpLine);
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
        while self.mlb_int.is_high().unwrap_or_default() || self.fw_wp.is_high().unwrap_or_default()
        {
            if self.get_millis() - mcu_start_ms > MCU_START_TIMEOUT_MS {
                if self.mlb_int.is_high().unwrap_or_default() {
                    error!(
                        "MCU failed to pull MLB_INT low after {}ms",
                        MCU_START_TIMEOUT_MS
                    );
                    return Err(Error::InterruptLine);
                } else {
                    error!(
                        "MCU failed to pull FW_WP low after {}ms",
                        MCU_START_TIMEOUT_MS
                    );
                    return Err(Error::FwwpLine);
                }
            }
        }

        // Wait for MCU to indicate that it has finished.
        // TODO: Add a timeout.
        while self.mlb_int.is_low().unwrap_or_default() {}

        // For some reason when we issue the the GO command, the I2C bus gets
        // stuck. Possibly the system bootloader doesn't ack the last transfer
        // prior to when it transfers control out. The I2C ISR register
        // indicates that the bus is BUSY. If we leave it like this, then our
        // attempt to read the test status from the HPS MCU blocks indefinitely.
        // Disabling then reenabling the I2C peripheral fixes this.
        {
            let p = unsafe { stm32::Peripherals::steal() };
            let i2c = p.I2C3;
            i2c.cr1.modify(|_, w| w.pe().clear_bit());
            i2c.cr1.modify(|_, w| w.pe().set_bit());
        }

        let status = self.read_status()?;

        if status.error_code != factory_tester_common::TEST_STATUS_OK {
            return Err(Error::try_from(status.error_code).unwrap_or(Error::Internal));
        }

        if !ALLOWED_SPI_FLASH_IDS.contains(&status.outputs.spi_flash_id) {
            error!(
                "Unsupported SPI flash ID 0x{:x}, [0x{:x}, 0x{:x}]",
                status.outputs.spi_flash_id.manufacturer_id,
                status.outputs.spi_flash_id.device_id[0],
                status.outputs.spi_flash_id.device_id[1]
            );
            return Err(Error::UnexpectedSpiFlashId);
        }

        Ok(())
    }

    fn read_status(&mut self) -> Result<TestStatus> {
        let mut buffer = [0u8; TEST_STATUS_SIZE];
        self.i2c
            .read(MCU_TESTER_I2C_ADDRESS, &mut buffer)
            .map_err(|e| {
                error!("Failed to read test result from MCU: {:?}", e);
                Error::Internal
            })?;
        Ok(TestStatus::from_bytes(&buffer))
    }

    /// Writes the test result to the MCU flash for later retrieval (once the
    /// HPS unit is installed into a laptop and has a network connection).
    fn log_result(&mut self, test_result: Result<()>) -> Result<()> {
        // 8 bytes would be a suitable size for a log entry, but unfortunately
        // if we write 8 bytes at a time, the writes seem to silently fail for
        // some addresses. It's not clear why. For now we use 128 bytes because
        // that seems to work reliably. Using such a large size entry reduces
        // the number of log results we can store, but it's still 512 results,
        // which is an absurd number of times to test a single device, so
        // shouldn't be a problem.
        const TEST_LOG_ENTRY_SIZE: usize = 128;

        let numeric_result = match test_result {
            Ok(_) => 0,
            Err(e) => e.into(),
        };

        let stm32_config =
            stm32_bootloader_client::Config::i2c_address(FACTORY_BOOTLOADER_I2C_ADDRESS);
        let mut stm32 = Stm32::new(&mut self.i2c, &mut self.delay, stm32_config);

        // Wait for the system bootloader to come back up after the MCU reset
        // itself.
        for _ in 0..100 {
            if stm32.get_chip_id().is_ok() {
                break;
            }
        }

        let mut buffer = [0u8; TEST_LOG_ENTRY_SIZE];
        for address in LOG_AREA.step_by(buffer.len()) {
            stm32.read_memory(address, &mut buffer).map_err(|e| {
                error!("Failed to read flash when logging result: {}", e);
                Error::SystemBootloader
            })?;

            if buffer == [0xff; TEST_LOG_ENTRY_SIZE] {
                // Put a somewhat arbitrary header in to help identify that this
                // is a test log.
                buffer[..4].copy_from_slice(b"TLOG");
                buffer[4..6].copy_from_slice(&TESTER_VERSION.to_le_bytes());
                buffer[6] = numeric_result;

                // Write the data back.
                stm32.write_memory(address, &buffer).map_err(|e| {
                    error!("Failed to write flash when logging result: {}", e);
                    Error::SystemBootloader
                })?;

                let mut read_back = [0u8; TEST_LOG_ENTRY_SIZE];
                stm32.read_memory(address, &mut read_back).map_err(|e| {
                    error!("Failed to read flash when logging result: {}", e);
                    Error::SystemBootloader
                })?;
                if read_back != buffer {
                    error!(
                        "Wrote: {:x?} to log at address 0x{:x}, but when read back, found: {:x?}",
                        buffer, address, read_back
                    );
                    return Err(Error::SystemBootloader);
                }

                return Ok(());
            }
        }

        warn!("Log area of MCU flash is full!");
        // We can't log anything more at this point, but we don't want the test
        // to fail because of that.
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
