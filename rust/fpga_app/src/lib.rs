// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

pub mod camera;
mod camera_tester;
mod classifier;
mod debug_commands;
mod entropy;
mod features;
mod histogram;
mod image_rotation;
mod mcu_interface;
mod timer;

use application::Rotation;
use application::Status;
pub use classifier::Classifier;
pub use classifier::FakeClassifier;
pub use classifier::TfLiteInitStatus;
use core::convert::TryFrom;
pub use debug_commands::DebugCommand;
use embedded_hal::blocking::spi;
pub use features::Features;
use hm01b0::TestPattern;
use mcu_interface::McuInterface;
pub use timer::Instant;
pub use timer::Timer;

const SOC_ROM_VERSION: u8 = 1;

// We do multiple tests per loop iteration for two reasons. (1) we report our
// status each loop iteration, so if we only did one test, then we'd be sending
// as many status reports as echo requests and (2) we'd overflow our loop
// counter too quickly if we only did a single test.
pub const MCU_COMMS_TESTS_PER_LOOP: u32 = 100;

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
            #[cfg(feature = "dev")]
            {
                use core::fmt::Write;
                let _ = writeln!($mcu.mcu_mut(), $($tts)*);
            }
            // If printing of any kind is disabled, then pass our arguments to
            // the formatter, but discard the result. This avoids warnings about
            // unused variables.
            #[cfg(not(feature = "dev"))]
            {
                let _ = format_args!($($tts)*);
            }
        }
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Camera,
    InvalidArg,
    McuTimeout,
    SelfTestFailed,
    SpiError,
    TfLiteFailure,
    UnknownFeature,
}

pub type Result<T> = core::result::Result<T, Error>;

pub struct App<S, I, C, T> {
    mcu: McuInterface<S>,
    image_data_receiver: I,
    classifier: C,
    #[cfg(feature = "image-transfer")]
    image_transfer_count: u32,
    classification_enabled: bool,
    debug_rotate_image: Option<bool>,
    calibration_bad_frames: u8,
    timer: T,
    status: Status,
    model_number: u32,
    spi_test_data: Option<&'static [u8]>,
}

impl<S, I, C, T> App<S, I, C, T>
where
    S: spi::Transfer<u8>,
    I: camera::ImageDataReceiver,
    C: Classifier,
    T: Timer,
{
    pub fn new(spi: S, image_data_receiver: I, classifier: C, timer: T) -> Self {
        Self {
            mcu: McuInterface::new(spi),
            image_data_receiver,
            classifier,
            #[cfg(feature = "image-transfer")]
            image_transfer_count: 0,
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
        let status = self.classifier.init_status();
        mcu_println!(self, "Classifier status: {:?}", status);
        if status != TfLiteInitStatus::InitOk {
            self.mcu.report_error(mcu_common::Error::TFLITE_FAILURE);
        }
        mcu_println!(self, "");
        if self.calibrate_camera(15).is_err() {
            mcu_println!(self, "Failed to calibrate camera");
            self.mcu.report_error(mcu_common::Error::CAMERA);
        }
        loop {
            if let Err(error) = self.loop_body() {
                mcu_println!(&mut self.mcu, "Error: {:?}", error);
                self.mcu.report_error(fpga_to_mcu_error(error));
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

        if !features.contains(Features::MODEL1) {
            self.status.person_status = 0;
        }
        if !features.contains(Features::MODEL2) {
            self.status.second_person_status = 0;
        }

        if features.contains(Features::SPI_FLASH_READ_TEST) {
            self.spi_flash_read_test();
        }
        if features.contains(Features::MCU_FPGA_COMM_TEST) {
            self.mcu_fpga_comm_test();
        }
        if features.contains(Features::CAMERA_DATA_TEST) {
            self.camera_data_test();
        }

        #[cfg(feature = "dev")]
        {
            self.process_debug_commands();
        }

        self.select_model(&features);

        self.check_calibration()?;
        self.capture_image(
            self.debug_rotate_image
                .unwrap_or_else(|| rotation == Rotation::Clockwise),
        )?;
        let image_data = self.classifier.input_data(self.model_number);
        let entropy = crate::entropy::calculate_entropy(image_data);
        mcu_println!(self.mcu, "{}", histogram::Histogram::new(image_data));

        // Image transfer can be enabled either via a feature (transfer over
        // I2C) or via a hps-mon debug command (transfer over RTT).
        #[cfg(feature = "image-transfer")]
        {
            if features.contains(Features::IMAGE_TRANSFER) || self.image_transfer_count != 0 {
                self.mcu
                    .send_image_data(self.classifier.input_data_mut(self.model_number))?;
            }
            if self.image_transfer_count != 0 && self.image_transfer_count != u32::MAX {
                self.image_transfer_count -= 1;
            }
        }

        // Classification needs to be done after any other uses of the image
        // (e.g. image transfer), since it overwrites the image buffer with
        // other data.
        let model_status;
        if entropy < crate::entropy::ENTROPY_THRESHOLD
            && (features.contains(Features::MODEL1)
                || features.contains(Features::MODEL2)
                || self.classification_enabled)
        {
            let start = self.timer.now();
            let score = self.classifier.run_model(self.model_number)?;
            model_status = 0x8000 | ((score as u16) & 0xff);
            mcu_println!(
                self.mcu,
                "Model {} score: {} took: {}ms",
                self.model_number,
                score,
                self.timer.elapsed_ms(start)
            );
        } else {
            model_status = 0;
        }
        if self.model_number == 1 {
            self.status.person_status = model_status;
        } else {
            self.status.second_person_status = model_status;
        }

        self.status.loop_count += 1;
        self.status.enabled_features = config.enabled_features;
        self.mcu.report_status(&self.status)?;

        Ok(())
    }

    fn calibrate_camera(&mut self, num_frames: u8) -> Result<()> {
        let start = self.timer.now();
        mcu_println!(self, "Calibrating camera");
        self.mcu.camera().set_ae_target(camera::AE_TARGET.into())?;
        self.mcu.wait_for_camera_standby();
        self.mcu.trigger_n_frames(num_frames)?;
        self.mcu.wait_for_camera_standby();
        mcu_println!(
            self.mcu,
            "Calibration complete in {}ms",
            self.timer.elapsed_ms(start)
        );
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
        let mcu = &mut self.mcu;
        mcu.wait_for_camera_standby();
        let buffer = self.classifier.input_data_mut(self.model_number);
        self.image_data_receiver
            .receive_image_data(buffer, &dimensions, || mcu.trigger_frame())?;
        if rotate_clockwise {
            image_rotation::rotate_240_320_clockwise(buffer);
        }
        Ok(())
    }

    #[cfg(feature = "dev")]
    fn process_debug_commands(&mut self) {
        while let Some((command, arg)) = self.mcu.next_debug_command() {
            // If all features are enabled then we exhaustively match, however
            // if some features are disabled then we need a catch-all.
            #[allow(unreachable_patterns)]
            match command {
                DebugCommand::Classification => {
                    let enable = arg != 0;
                    self.classification_enabled = enable;
                    mcu_println!(
                        self,
                        "Classification enabled {}",
                        if enable { "enabled" } else { "disabled" }
                    );
                }
                DebugCommand::RotateImageClockwise => {
                    let enable = arg != 0;
                    self.debug_rotate_image = Some(enable);
                    mcu_println!(
                        self,
                        "Rotation enabled {}",
                        if enable { "enabled" } else { "disabled" }
                    );
                }
                #[cfg(feature = "image-transfer")]
                DebugCommand::Transfer => {
                    let enable = arg != 0;
                    self.image_transfer_count = if enable { 0 } else { u32::MAX };
                    mcu_println!(
                        self,
                        "Image transfer: {}",
                        if enable { "enabled" } else { "disabled" }
                    );
                }
                #[cfg(feature = "image-transfer")]
                DebugCommand::TransferCount => {
                    self.image_transfer_count = arg as u32;
                    mcu_println!(self, "Image transfer enabled ({} images)", arg);
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

    #[cfg(feature = "dev")]
    fn self_test(&mut self) -> Result<()> {
        if !cfg!(feature = "self-test") {
            mcu_println!(self, "Self-test feature not enabled");
            return Err(Error::UnknownFeature);
        }
        const CAT: &[u8] = include_bytes!("../test_data/cat_picture.dat");

        mcu_println!(self, "Testing TFLM on canned data");

        self.check_score(1, "presence", CAT, -101)?;
        self.check_score(2, "second", CAT, -117)?;

        mcu_println!(self, "Self-test passed");
        Ok(())
    }

    #[cfg(feature = "dev")]
    fn check_score(
        &mut self,
        model_number: u32,
        name: &str,
        input_data: &[u8],
        expected: i8,
    ) -> Result<()> {
        let buffer = self.classifier.input_data_mut(model_number);
        for i in 0..buffer.len() {
            buffer[i] = (i16::from(input_data[i]) - 128) as u8;
        }
        let start = self.timer.now();
        let score = self.classifier.run_model(model_number)?;
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
        loop {
            for _ in 0..MCU_COMMS_TESTS_PER_LOOP {
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

    fn camera_data_test(&mut self) {
        let mut bad = 0;
        self.status.loop_count = 0;
        loop {
            self.status.loop_count += 1;
            if self.capture_and_check_test_pattern().is_err() {
                bad += 1;
            }

            self.status.person_status = bad;
            let _ = self.mcu.report_status(&self.status);
        }
    }

    fn capture_and_check_test_pattern(&mut self) -> Result<()> {
        // We use the color-bar test pattern because it's more consistent than
        // walking 1s. Some frames with walking-1s end up never having a 1 in
        // some bit position, which means we can't reliably test all data lines.
        self.mcu
            .camera()
            .set_test_pattern(Some(TestPattern::ColorBar))?;
        self.capture_image(false)?;
        camera_tester::check_test_pattern(self.classifier.input_data_mut(self.model_number))?;
        Ok(())
    }

    /// Selects which model to use for the current loop iteration so that each
    /// enabled feature gets a turn. If no model features are enabled, will
    /// select model 1.
    fn select_model(&mut self, features: &Features) {
        // Advance to next model number out of 1 and 2.
        self.model_number = (self.model_number % 2) + 1;

        // If debug classification enabled, run both models
        if self.classification_enabled {
            return;
        }

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

fn fpga_to_mcu_error(error: Error) -> mcu_common::Error {
    // TODO: Get rid of our own Error enum and just use
    // mcu_common::Error everywhere.
    match error {
        Error::Camera => mcu_common::Error::CAMERA,
        Error::InvalidArg => mcu_common::Error::HOST_I2C_BAD_REQUEST,
        Error::McuTimeout => mcu_common::Error::FPGA_MCU_COMM_ERROR,
        Error::SelfTestFailed => mcu_common::Error::SELF_TEST_FAILED,
        Error::SpiError => mcu_common::Error::FPGA_MCU_COMM_ERROR,
        Error::TfLiteFailure => mcu_common::Error::TFLITE_FAILURE,
        Error::UnknownFeature => mcu_common::Error::HOST_I2C_BAD_REQUEST,
    }
}

#[cfg(test)]
mod tests {
    use crate::camera::testing::FakeImageDataReceiver;
    use crate::timer::testing::FakeTimer;
    use application::fpga;
    use application::fpga::commands::Action;
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
                vec![Action::ReadConfigRegisters.into()],
                config.to_bytes().to_vec(),
            )
        }

        fn expect_read_camera_reg(self, register: u16, value: u8) -> Self {
            let register_bytes = register.to_be_bytes();
            self.expect(
                vec![
                    Action::ReadCameraI2c.into(),
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
                    Action::WriteCameraI2c.into(),
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
            request.insert(0, Action::WriteStatusRegisters.into());
            self.expect(request, vec![])
        }

        fn build(self) -> embedded_hal_mock::spi::Mock {
            embedded_hal_mock::spi::Mock::new(&self.expectations)
        }
    }

    #[test]
    fn test_feature_toggle() {
        let mut classifier = FakeClassifier::default();
        classifier.model_outputs[0] = 0x42;
        let config = Configuration {
            enabled_features: (Features::MODEL1 | Features::MODEL2).bits(),
            camera_config: Rotation::Clockwise.into(),
        };
        let status1 = Status {
            loop_count: 1,
            person_status: 0,
            second_person_status: 0x8042,
            enabled_features: config.enabled_features,
        };
        let status2 = Status {
            person_status: 0x8042,
            loop_count: 2,
            ..status1
        };
        let config_disabled = Configuration {
            enabled_features: Features::DISABLED.bits(),
            camera_config: Rotation::Clockwise.into(),
        };
        let status3 = Status {
            loop_count: 3,
            person_status: 0,
            second_person_status: 0,
            enabled_features: config_disabled.enabled_features,
        };
        let status4 = Status {
            person_status: 0,
            loop_count: 4,
            ..status3
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
            .expect_report_status(&status1)
            .expect_get_config(&config)
            // Get AE mean.
            .expect_read_camera_reg(0x2020, 100)
            // Wait camera standby.
            .expect_read_camera_reg(0x0100, 0)
            // Configure number of frames.
            .expect_write_camera_reg(0x3020, 1)
            // Stream N frames.
            .expect_write_camera_reg(0x0100, 3)
            .expect_report_status(&status2)
            .expect_get_config(&config_disabled)
            // Get AE mean.
            .expect_read_camera_reg(0x2020, 100)
            // Wait camera standby.
            .expect_read_camera_reg(0x0100, 0)
            // Configure number of frames.
            .expect_write_camera_reg(0x3020, 1)
            // Stream N frames.
            .expect_write_camera_reg(0x0100, 3)
            .expect_report_status(&status3)
            .expect_get_config(&config_disabled)
            // Get AE mean.
            .expect_read_camera_reg(0x2020, 100)
            // Wait camera standby.
            .expect_read_camera_reg(0x0100, 0)
            // Configure number of frames.
            .expect_write_camera_reg(0x3020, 1)
            // Stream N frames.
            .expect_write_camera_reg(0x0100, 3)
            .expect_report_status(&status4)
            .build();
        let mut app = App::new(spi, FakeImageDataReceiver, classifier, FakeTimer);

        assert_eq!(app.loop_body(), Ok(()));
        assert_eq!(app.loop_body(), Ok(()));
        assert_eq!(app.classifier.models_executed, 1 | 2);
        assert_eq!(app.status.person_status, 0x8042);
        assert_eq!(app.status.second_person_status, 0x8042);
        assert_eq!(app.loop_body(), Ok(()));
        assert_eq!(app.loop_body(), Ok(()));
        assert_eq!(app.status.person_status, 0);
        assert_eq!(app.status.second_person_status, 0);
        assert_eq!(app.classifier.models_executed, 1 | 2);

        app.mcu.spi.done();
    }
}
