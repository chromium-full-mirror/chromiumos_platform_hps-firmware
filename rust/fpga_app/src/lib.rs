// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

pub mod camera;
mod classifier;
mod debug_commands;
mod debug_led;
mod features;
mod image_rotation;
mod mcu_interface;
mod timer;

use application::Rotation;
use application::Status;
pub use classifier::Classifier;
pub use classifier::FakeClassifier;
use core::convert::TryFrom;
use core::fmt::Write;
pub use debug_commands::DebugCommand;
pub use debug_led::DebugLed;
use embedded_hal::blocking::spi;
pub use features::Features;
use mcu_interface::McuInterface;
pub use timer::Instant;
pub use timer::Timer;

const SOC_ROM_VERSION: u8 = 1;

/// Print to hps-mon via the MCU. The first argument can be anything that
/// implements `mcu_mut()`.
macro_rules! mcu_println {
    ($mcu:expr, $($tts:tt)*) => {
        #[cfg(test)]
        {
            println!($($tts)*);
        }
        #[cfg(not(test))]
        {
            let _ = writeln!($mcu.mcu_mut(), $($tts)*);
        }
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    SpiError,
    McuTimeout,
    InvalidArg,
    UnknownFeature,
    SelfTestFailed,
    Camera,
}

pub type Result<T> = core::result::Result<T, Error>;

pub struct App<S, I, L, C, T> {
    mcu: McuInterface<S>,
    image_data_receiver: I,
    classifier: C,
    led: L,
    #[cfg(feature = "image-transfer")]
    image_transfer_enabled: bool,
    classification_enabled: bool,
    debug_rotate_image: Option<bool>,
    calibration_bad_frames: u8,
    timer: T,
    status: Status,
    model_number: u32,
    spi_test_data: Option<&'static [u8]>,
}

impl<S, I, L, C, T> App<S, I, L, C, T>
where
    S: spi::Transfer<u8>,
    I: camera::ImageDataReceiver,
    L: DebugLed,
    C: Classifier,
    T: Timer,
{
    pub fn new(spi: S, image_data_receiver: I, classifier: C, led: L, timer: T) -> Self {
        Self {
            mcu: McuInterface::new(spi),
            image_data_receiver,
            classifier,
            led,
            #[cfg(feature = "image-transfer")]
            image_transfer_enabled: false,
            classification_enabled: false,
            debug_rotate_image: None,
            calibration_bad_frames: 0,
            timer,
            status: Status::default(),
            model_number: 1,
            spi_test_data: None,
        }
    }

    pub fn run(&mut self) -> ! {
        self.mcu.resync();
        let _ = self.mcu.report_boot(SOC_ROM_VERSION);
        mcu_println!(self, "Hello from the Rust FPGA");
        self.led.turn_on();
        if self.calibrate_camera(15).is_err() {
            mcu_println!(self, "Failed to calibrate camera");
        }
        loop {
            if let Err(error) = self.loop_body() {
                let _ = writeln!(&mut self.mcu, "Error: {:?}", error);
            }
        }
    }

    pub fn set_spi_test_data(&mut self, region: &'static [u8]) {
        self.spi_test_data = Some(region);
    }

    pub fn mcu_mut(&mut self) -> &mut McuInterface<S> {
        &mut self.mcu
    }

    fn loop_body(&mut self) -> Result<()> {
        let config = self.mcu.read_config()?;
        let features = Features::from_bits(config.enabled_features).ok_or(Error::UnknownFeature)?;
        let rotation =
            Rotation::try_from(config.camera_config & 3).map_err(|_| Error::InvalidArg)?;

        if features.contains(Features::SPI_FLASH_READ_TEST) {
            self.spi_flash_read_test();
        }

        if cfg!(feature = "debug-commands") {
            self.process_debug_commands();
        }

        if features.contains(Features::DEBUG_LED) {
            self.led.turn_on();
        } else {
            self.led.turn_off();
        }

        self.select_model(&features);

        self.check_calibration()?;
        self.capture_image(
            self.debug_rotate_image
                .unwrap_or_else(|| rotation == Rotation::Clockwise),
        )?;

        // Image transfer can be enabled either via a feature (transfer over
        // I2C) or via a hps-mon debug command (transfer over RTT).
        #[cfg(feature = "image-transfer")]
        {
            if features.contains(Features::IMAGE_TRANSFER) || self.image_transfer_enabled {
                self.mcu
                    .send_image_data(self.classifier.input_data_mut(self.model_number))?;
            }
        }

        // Classification needs to be done after any other uses of the image
        // (e.g. image transfer), since it overwrites the image buffer with
        // other data.
        if features.contains(Features::MODEL1)
            || features.contains(Features::MODEL2)
            || self.classification_enabled
        {
            let start = self.timer.now();
            let score = self.classifier.run_model(self.model_number);
            // TODO: integrate image usability code and use that to set the
            // usable bit.
            self.status.person_status = 0x8000 | ((score as u16) & 0xff);
            mcu_println!(
                self.mcu,
                "Model {} score: {} took: {}ms",
                self.model_number,
                score,
                self.timer.elapsed_ms(start)
            );
        } else {
            self.status.person_status = 0;
        }

        self.status.loop_count += 1;
        self.status.enabled_features = config.enabled_features;
        self.mcu.report_status(&self.status)?;

        Ok(())
    }

    fn calibrate_camera(&mut self, num_frames: u8) -> Result<()> {
        mcu_println!(self, "Calibrating camera");
        self.mcu.camera().set_ae_target(camera::AE_TARGET.into())?;
        self.mcu.wait_for_camera_standby();
        self.mcu.trigger_n_frames(num_frames)?;
        self.mcu.wait_for_camera_standby();
        mcu_println!(self, "Calibration complete");
        Ok(())
    }

    fn check_calibration(&mut self) -> Result<()> {
        let mean = self.mcu.camera().get_ae_mean()?;
        if (mean as i16 - camera::AE_TARGET as i16).abs() > camera::AE_RECALIBRATION_THRESHOLD {
            self.calibration_bad_frames += 1;
            if self.calibration_bad_frames >= 5 {
                self.calibrate_camera(5)?;
                self.calibration_bad_frames = 0;
            }
        } else {
            self.calibration_bad_frames = 0;
        }
        Ok(())
    }

    fn capture_image(&mut self, rotate_clockwise: bool) -> Result<()> {
        let mut dimensions = camera::ImageDimensions {
            column_start: 2,
            column_count: camera::IMAGE_WIDTH,
            row_start: (324 - camera::IMAGE_HEIGHT) / 2,
            row_count: camera::IMAGE_HEIGHT,
        };
        if rotate_clockwise {
            core::mem::swap(&mut dimensions.column_start, &mut dimensions.row_start);
            core::mem::swap(&mut dimensions.column_count, &mut dimensions.row_count);
        }
        let start = self.timer.now();
        let mcu = &mut self.mcu;
        mcu.wait_for_camera_standby();
        let buffer = self.classifier.input_data_mut(self.model_number);
        self.image_data_receiver
            .receive_image_data(buffer, &dimensions, || mcu.trigger_frame())?;
        if rotate_clockwise {
            image_rotation::rotate_240_320_clockwise(buffer);
        }
        mcu_println!(mcu, "Image captured in {}ms", self.timer.elapsed_ms(start));
        Ok(())
    }

    fn process_debug_commands(&mut self) {
        while let Some((command, arg)) = self.mcu.next_debug_command() {
            // If all features are enabled then we exhaustively match, however
            // if some features are disabled then we need a catch-all.
            #[allow(unreachable_patterns)]
            match command {
                DebugCommand::ClassificationOn => {
                    self.classification_enabled = true;
                    mcu_println!(self, "Classification enabled");
                }
                DebugCommand::ClassificationOff => {
                    self.classification_enabled = false;
                    mcu_println!(self, "Classification disabled");
                }
                DebugCommand::RotateImageClockwiseOn => {
                    self.debug_rotate_image = Some(true);
                    mcu_println!(self, "Rotation enabled");
                }
                DebugCommand::RotateImageClockwiseOff => {
                    self.debug_rotate_image = Some(false);
                    mcu_println!(self, "Rotation disabled");
                }
                #[cfg(feature = "image-transfer")]
                DebugCommand::TransferOn => {
                    self.image_transfer_enabled = true;
                    mcu_println!(self, "Image transfer enabled");
                }
                #[cfg(feature = "image-transfer")]
                DebugCommand::TransferOff => {
                    self.image_transfer_enabled = false;
                    mcu_println!(self, "Image transfer disabled");
                }
                DebugCommand::ExposureCalibration => {
                    if self.calibrate_camera(5).is_err() {
                        mcu_println!(self, "Failed to calibrate camera");
                    }
                }
                DebugCommand::SelfTest => {
                    if self.self_test().is_err() {
                        mcu_println!(self, "Self-test failed");
                    }
                }
                DebugCommand::TestSpiFlashReads => {
                    mcu_println!(self, "Performing SPI flash read integrity test...");
                    self.spi_flash_read_test();
                }
                DebugCommand::TestFpgaMcuComms => {
                    mcu_println!(self, "Performing FPGA<->MCU communication test...");
                    self.mcu_fpga_comm_test();
                }
                _ => {
                    mcu_println!(self, "DebugCommand::{:?}({}) not implemented", command, arg);
                }
            }
        }
    }

    fn self_test(&mut self) -> Result<()> {
        if !cfg!(feature = "self-test") {
            mcu_println!(self, "Self-test feature not enabled");
            return Err(Error::UnknownFeature);
        }
        const NOT_PERSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/not-person.raw"));
        const PERSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/person.raw"));

        mcu_println!(self, "Testing TFLM on canned data");

        self.check_score("person", PERSON, 126)?;
        self.check_score("not-person", NOT_PERSON, -127)?;

        mcu_println!(self, "Self-test passed");
        Ok(())
    }

    fn check_score(&mut self, name: &str, input_data: &[u8], expected: i8) -> Result<()> {
        let model_number = 1;
        self.classifier
            .input_data_mut(model_number)
            .copy_from_slice(input_data);
        let start = self.timer.now();
        let score = self.classifier.run_model(model_number);
        let elapsed = self.timer.elapsed_ms(start);
        if score == expected {
            mcu_println!(self, "Got expected result {} in {}ms", expected, elapsed);
        } else {
            mcu_println!(
                self,
                "Unexpected presence score for {}: Expected {}, got {}",
                name,
                expected,
                score
            );
            return Err(Error::SelfTestFailed);
        }
        Ok(())
    }

    /// Repeatedly reads some test data that we've previously written to the SPI
    /// flash and checks it against the expected pattern, reporting the number
    /// of iteration as well as the number of bad reads we've encountered.
    fn spi_flash_read_test(&mut self) {
        let test_data = match self.spi_test_data {
            Some(x) => x,
            None => {
                mcu_println!(self, "SPI test region not supplied");
                return;
            }
        };
        let mut bad = 0;
        self.status.loop_count = 0;
        loop {
            for chunk in test_data.chunks_exact(256) {
                for (offset, byte) in chunk.iter().enumerate() {
                    if *byte != offset as u8 {
                        bad += 1;
                    }
                }
            }
            self.status.loop_count += 1;
            if self.status.loop_count & 0xf == 0 {
                mcu_println!(
                    self.mcu,
                    "{} iterations with {} errors so far",
                    self.status.loop_count,
                    bad
                );
            }
            self.status.person_status = bad;
            let _ = self.mcu.report_status(&self.status);
        }
    }

    fn mcu_fpga_comm_test(&mut self) {
        let mut bad = 0;
        self.status.loop_count = 0;
        // We do multiple tests per loop iteration for two reasons. (1) we
        // report our status each loop iteration, so if we only did one test,
        // then we'd be sending as many status reports as echo requests and (2)
        // we'd overflow our loop counter too quickly if we only did a single
        // test.
        const TESTS_PER_LOOP: u32 = 100;
        loop {
            for _ in 0..TESTS_PER_LOOP {
                if self.mcu.echo_test().is_err() {
                    bad += 1;
                }
            }
            self.status.loop_count += 1;
            self.status.person_status = bad;
            let _ = self.mcu.report_status(&self.status);
            mcu_println!(
                self.mcu,
                "{} iterations with {} errors so far",
                self.status.loop_count,
                bad
            );
        }
    }

    /// Selects which model to use for the current loop iteration so that each
    /// enabled feature gets a turn. If no model features are enabled, will
    /// select model 1.
    fn select_model(&mut self, features: &Features) {
        // Advance to next model number out of 1 and 2.
        self.model_number = (self.model_number % 2) + 1;
        // If we're on model 1, but that's not active, advance to model 2.
        if self.model_number == 1 && !features.contains(Features::MODEL1) {
            self.model_number = 2;
        }
        // If we're on model 2, but that's not active, advance to model 1. Note,
        // this means that if neither model is active, we'll use model 1. It
        // won't actually run, but we need a model to always be selected so that
        // we have a buffer into which to capture images.
        if self.model_number == 2 && !features.contains(Features::MODEL2) {
            self.model_number = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::camera::testing::FakeImageDataReceiver;
    use crate::debug_led::testing::FakeLed;
    use crate::timer::testing::FakeTimer;
    use application::fpga;
    use application::Configuration;

    use super::*;

    #[derive(Default)]
    struct SpiExpectationsBuilder {
        expectations: Vec<embedded_hal_mock::spi::Transaction>,
    }

    fn empty() -> Vec<u8> {
        vec![0u8; fpga::BUFFER_SIZE]
    }

    impl SpiExpectationsBuilder {
        fn expect(mut self, mut request: Vec<u8>, mut response: Vec<u8>) -> Self {
            assert!(!request.is_empty());
            response.insert(0, fpga::status::OK);
            response.insert(1, request[0]);
            assert!(request.len() <= fpga::BUFFER_SIZE);
            assert!(response.len() <= fpga::BUFFER_SIZE);
            request.resize(fpga::BUFFER_SIZE, 0);
            response.resize(fpga::BUFFER_SIZE, 0);
            self.expectations
                .push(embedded_hal_mock::spi::Transaction::transfer(
                    request,
                    empty(),
                ));
            self.expectations
                .push(embedded_hal_mock::spi::Transaction::transfer(
                    empty(),
                    response,
                ));
            self
        }

        fn expect_get_config(self, config: &Configuration) -> Self {
            self.expect(
                vec![fpga::commands::READ_CONFIG_REGISTERS],
                config.to_bytes().to_vec(),
            )
        }

        fn expect_read_camera_reg(self, register: u16, value: u8) -> Self {
            let register_bytes = register.to_be_bytes();
            self.expect(
                vec![
                    fpga::commands::READ_CAMERA_I2C,
                    1,
                    register_bytes[0],
                    register_bytes[1],
                ],
                vec![value],
            )
        }

        fn expect_write_camera_reg(self, register: u16, value: u8) -> Self {
            let register_bytes = register.to_be_bytes();
            self.expect(
                vec![
                    fpga::commands::WRITE_CAMERA_I2C,
                    1,
                    register_bytes[0],
                    register_bytes[1],
                    value,
                ],
                vec![],
            )
        }

        fn expect_report_status(self, status: &Status) -> Self {
            let mut request = status.to_bytes().to_vec();
            request.insert(0, fpga::commands::WRITE_STATUS_REGISTERS);
            self.expect(request, vec![])
        }

        fn build(self) -> embedded_hal_mock::spi::Mock {
            embedded_hal_mock::spi::Mock::new(&self.expectations)
        }
    }

    #[test]
    fn test_feature_enabled() {
        let mut classifier = FakeClassifier::default();
        classifier.model_outputs[0] = 0x42;
        let config = Configuration {
            enabled_features: Features::MODEL1.bits(),
            camera_config: Rotation::Clockwise.into(),
        };
        let status = Status {
            loop_count: 1,
            person_status: 0x8042,
            enabled_features: config.enabled_features,
            ..Default::default()
        };
        let spi = SpiExpectationsBuilder::default()
            .expect_get_config(&config)
            // Get AE mean.
            .expect_read_camera_reg(0x2020, 100)
            // Wait camera standby.
            .expect_read_camera_reg(0x0100, 0)
            // Configure number of frames.
            .expect_write_camera_reg(0x3020, 1)
            // Stream N frames.
            .expect_write_camera_reg(0x0100, 3)
            .expect_report_status(&status)
            .build();
        let mut app = App::new(spi, FakeImageDataReceiver, classifier, FakeLed, FakeTimer);

        assert_eq!(app.loop_body(), Ok(()));

        assert_eq!(app.classifier.models_executed, 1);

        app.mcu.spi.done();
    }
}
