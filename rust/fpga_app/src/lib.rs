// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

pub mod camera;
mod camera_tester;
mod classifier;
mod debug_commands;
mod features;
mod histogram;
mod image_rotation;
mod mcu_interface;
mod timer;

#[cfg(test)]
mod fake_mcu;
#[cfg(test)]
mod test_helpers;

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
    histogram_enabled: bool,
    timer: T,
    status: Status,
    model_number: u32,
    spi_test_data: Option<&'static [u8]>,
    exposure_median_target: Option<i8>,
    exposure: i32,
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
            histogram_enabled: false,
            timer,
            status: Status::default(),
            model_number: 1,
            spi_test_data: None,
            exposure_median_target: Some(-64),
            exposure: 0x100,
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
        if let Err(error) = self.camera_setup() {
            mcu_println!(self, "Camera error: {:?}", error);
            self.mcu.report_error(fpga_to_mcu_error(error));
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

        let is_rotated = self.determine_rotation(&config)?;
        self.capture_well_exposed_image(is_rotated)?;
        let image_data = self.classifier.input_data(self.model_number);
        let histogram = histogram::Histogram::new(image_data);
        if self.histogram_enabled {
            let e = self.exposure;
            mcu_println!(self.mcu, "{} exp: {:#06x}", histogram, e);
        }

        // Image transfer can be enabled either via a feature (transfer over
        // I2C) or via a hps-mon debug command (transfer over RTT).
        #[cfg(feature = "image-transfer")]
        {
            if features.contains(Features::IMAGE_TRANSFER) || self.image_transfer_count != 0 {
                self.mcu
                    .send_image_data(self.classifier.input_data(self.model_number))?;
            }
            if self.image_transfer_count != 0 && self.image_transfer_count != u32::MAX {
                self.image_transfer_count -= 1;
            }
        }

        // Classification needs to be done after any other uses of the image
        // (e.g. image transfer), since it overwrites the image buffer with
        // other data.
        let model_status;
        if histogram.is_usable()
            && (features.contains(Features::MODEL1) || features.contains(Features::MODEL2))
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

    fn camera_setup(&mut self) -> Result<()> {
        let mut camera = self.mcu.camera();

        // Disable AE. We'll control exposure programatically instead.
        camera.set_ae(false)?;
        // Set frame rate to ~21Hz by doubling frame length
        camera.set_frame_length(2 * 0x158)?;
        // Set camera to stream
        camera.set_mode(hm01b0::Mode::Streaming)?;

        // Writes the initial exposure configuration
        self.write_exposure()?;

        Ok(())
    }

    // Determine what the rotation setting is
    fn determine_rotation(&mut self, config: &application::Configuration) -> Result<bool> {
        Ok(
            Rotation::try_from(config.camera_config & 3).map_err(|_| Error::InvalidArg)?
                == Rotation::Clockwise,
        )
    }

    fn capture_well_exposed_image(&mut self, rotate_clockwise: bool) -> Result<()> {
        // If we don't have a target, then probably the exposure is being
        // manually controlled for debug purposes, so just write the set
        // value.
        if self.exposure_median_target == None {
            self.write_exposure()?;
            return self.capture_image(rotate_clockwise);
        }

        // Capture a first image
        self.capture_image(rotate_clockwise)?;

        // Algorithm will attempt to converge median on this value
        let target = self.exposure_median_target.unwrap() as i32;

        const BEGIN_THRESHOLD: i32 = 20;
        const END_THRESHOLD: i32 = 5;
        const MAX_STEPS: usize = 15;
        let mut median = self.get_image_median() as i32;
        let mut diff = median - target;
        if diff.abs() > BEGIN_THRESHOLD {
            let done_down = &|median| median - target < END_THRESHOLD;
            let done_up = &|median| median - target > -END_THRESHOLD;
            let done: &dyn Fn(i32) -> bool = if diff > 0 { done_down } else { done_up };
            let mut steps = 0;
            while !done(median) && steps < MAX_STEPS {
                self.step_exposure(if diff < 0 { 64 } else { -64 })?;
                self.capture_image(false)?;
                self.capture_image(rotate_clockwise)?;
                median = self.get_image_median() as i32;
                diff = median - target;
                steps += 1;
            }
        }

        // Always take a small step toward the median anyway
        self.step_exposure(if diff < 0 { 16 } else { -16 })?;
        Ok(())
    }

    // Move exposure by one step, up or down
    fn step_exposure(&mut self, size: i32) -> Result<()> {
        // Make delta dependent on current value
        let val = self.exposure;
        let delta =
            i32::max(1, i32::min(0x100, 0x100 * val * size.abs() / 0x10000)) * size.signum();
        // Clamp result to allowed range
        self.exposure = (val + delta).clamp(camera::MIN_EXPOSURE, camera::MAX_EXPOSURE);
        self.write_exposure()?;
        Ok(())
    }

    // Calculate median of current image buffer
    fn get_image_median(&self) -> i8 {
        let image_data = self.classifier.input_data(self.model_number);
        histogram::Histogram::new(image_data).median()
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
        let buffer = self.classifier.input_data_mut(self.model_number);
        self.image_data_receiver
            .receive_image_data(buffer, &dimensions, || Ok(()))?;
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
                #[cfg(feature = "image-transfer")]
                DebugCommand::Transfer => {
                    self.image_transfer_count = if arg != 0 { u32::MAX } else { 0 };
                    mcu_println!(self, "Image transfer: {}", arg != 0);
                }
                #[cfg(feature = "image-transfer")]
                DebugCommand::TransferCount => {
                    self.image_transfer_count = arg as u32;
                    mcu_println!(self, "Image transfer enabled ({} images)", arg);
                }
                DebugCommand::Histogram => {
                    self.histogram_enabled = arg != 0;
                    mcu_println!(self, "Histogram: {}", arg != 0);
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
                DebugCommand::SetExposure => {
                    mcu_println!(self, "Exposure: {}", arg);
                    self.exposure = arg as i32;
                }
                DebugCommand::SetMedianTarget => {
                    // if target out of range, turn auto exposure off
                    if (arg as i8 as i16) == arg as i16 {
                        self.exposure_median_target = Some(arg as i8);
                    } else {
                        self.exposure_median_target = None;
                    }
                    let t = self.exposure_median_target;
                    mcu_println!(self, "Exposure Median Target: {:?}", t);
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
            buffer[i] = (i16::from(input_data[i]) - 128) as i8;
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

    /// Writes exposure settings to the camera, assuming they are set
    pub fn write_exposure(&mut self) -> Result<()> {
        let e = camera::calculate_exposure(self.exposure);
        Ok(self.mcu.camera().set_exposure(&e)?)
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
    use crate::fake_mcu::FakeMcu;
    use crate::timer::testing::FakeTimer;
    use application::Configuration;

    use super::*;

    #[test]
    fn test_feature_toggle_nomiri() -> Result<()> {
        let mut app = App::new(
            FakeMcu::default(),
            FakeImageDataReceiver,
            FakeClassifier::default(),
            FakeTimer,
        );

        // Turn exposure control off to avoid trying to change exposure of camera fakes.
        app.exposure_median_target = None;

        // Iteration 1: Both models enabled - second person model runs.
        app.classifier.model_outputs[0] = 0x42;
        app.mcu.spi.state.configuration = Configuration {
            enabled_features: (Features::MODEL1 | Features::MODEL2).bits(),
            camera_config: Rotation::Clockwise.into(),
        };
        assert_eq!(app.loop_body(), Ok(()));
        let status1 = Status {
            loop_count: 1,
            person_status: 0,
            second_person_status: 0x8042,
            enabled_features: app.mcu.spi.state.configuration.enabled_features,
        };
        assert_eq!(app.mcu.spi.state.status, status1);
        assert_eq!(app.classifier.models_executed, 2);

        // Iteration 2: Both models enabled - presence detection runs.
        app.classifier.model_outputs[0] = 0x73;
        app.classifier.models_executed = 0;
        assert_eq!(app.loop_body(), Ok(()));
        let status2 = Status {
            person_status: 0x8073,
            loop_count: 2,
            ..status1
        };
        assert_eq!(app.mcu.spi.state.status, status2);
        assert_eq!(app.classifier.models_executed, 1);
        assert_eq!(app.status.person_status, 0x8073);
        assert_eq!(app.status.second_person_status, 0x8042);

        // Iteration 3: Both models disabled.
        app.classifier.models_executed = 0;
        app.mcu.spi.state.configuration = Configuration {
            enabled_features: Features::DISABLED.bits(),
            camera_config: Rotation::Clockwise.into(),
        };
        assert_eq!(app.loop_body(), Ok(()));
        let status3 = Status {
            loop_count: 3,
            person_status: 0,
            second_person_status: 0,
            enabled_features: app.mcu.spi.state.configuration.enabled_features,
        };
        assert_eq!(app.mcu.spi.state.status, status3);
        assert_eq!(app.classifier.models_executed, 0);

        Ok(())
    }
}
