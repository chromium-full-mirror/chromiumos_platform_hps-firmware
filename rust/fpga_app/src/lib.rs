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

use application::Configuration;
use application::Rotation;
use application::Status;
use camera::ImageDataReceiver;
use camera::IMAGE_HEIGHT;
use camera::IMAGE_WIDTH;
pub use classifier::Classifier;
pub use classifier::FakeClassifier;
pub use classifier::TfLiteInitStatus;
use core::convert::TryFrom;
pub use debug_commands::DebugCommand;
use embedded_hal::blocking::spi;
pub use features::Features;
pub use histogram::Histogram;
use hm01b0::TestPattern;
use mcu_common::SPI_BLOCK_SIZE;
use mcu_interface::McuInterface;
pub use timer::Instant;
pub use timer::Microseconds;
pub use timer::Milliseconds;
pub use timer::Timer;

const SOC_ROM_VERSION: u8 = 1;

// We do multiple tests per loop iteration for two reasons. (1) we report our
// status each loop iteration, so if we only did one test, then we'd be sending
// as many status reports as echo requests and (2) we'd overflow our loop
// counter too quickly if we only did a single test.
pub const MCU_COMMS_TESTS_PER_LOOP: u32 = 100;

/// Offset into SPI flash at which we may store test images.
pub const TEST_IMAGES_OFFSET: u32 = 8 * 1024 * 1024;

/// Size of the area into which test areas are written.
pub const TEST_IMAGES_MAX_SIZE: u32 = 2 * 1024 * 1024;

/// Number of bytes in an image.
pub const IMAGE_SIZE: usize = (IMAGE_WIDTH * IMAGE_HEIGHT) as usize;

/// Alignment of each image in the test image area. We round up to the next SPI
/// block size, so that each image starts at the start of a block, making
/// writing of images easier.
pub const TEST_IMAGE_BLOCK_SIZE: usize =
    ((IMAGE_SIZE - 1) / SPI_BLOCK_SIZE as usize + 1) * SPI_BLOCK_SIZE as usize;

const NUM_MODELS: usize = 2;

/// Initial delay from when we start running a model until when we need the next
/// camera frame to be ready. This should ideally be >= classification time for
/// all models, although not too much more. Regardless, we'll automatically
/// adjust the trigger delay based on actual timings, so this is only a starting
/// value.
const INITIAL_FRAME_TRIGGER_DELAY: Microseconds = Microseconds(300_000);

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
    image_data_receiver: ImageDataReceiver<I, T>,
    classifier: C,
    #[cfg(feature = "image-transfer")]
    image_transfer_count: u32,
    histogram_enabled: bool,
    timer: T,
    status: Status,
    model_number: u32,
    model_result_count: [u8; NUM_MODELS],
    /// The model number, if any, that was run on the last iteration of the
    /// loop. Used to determine which element of trigger_delay_by_model to
    /// update.
    last_model_number: Option<u32>,
    /// Whether we've triggered a frame that we're still waiting to receive.
    frame_pending: bool,
    /// How long we'll wait before triggering a camera frame when we're about to
    /// run a particular model.
    trigger_delay_by_model: [Microseconds; NUM_MODELS],
    spi_test_data: Option<&'static [u8]>,
    test_image_area: Option<&'static [u8]>,
    exposure_algorithm: ExposureAlgorithm,
    exposure: i32,
}

struct ImageMetadata {
    /// How long we waited for the frame to start.
    wait_start: Microseconds,
    /// Whether we missed the start of our frame, or no frame had previously
    /// been requested.
    missed_frame: bool,
}

#[derive(Debug, Clone, Copy)]
enum ExposureAlgorithm {
    /// Exposure will be adjusted to try and get the median brightness near to
    /// the specified value.
    MedianTarget(i8),

    /// Let the camera do automatic exposure with the specified target.
    #[cfg(feature = "dev")]
    HardwareAe(u16),

    /// All exposure settings will be set based on the specified exposure value
    /// (no auto adjustment).
    #[cfg(feature = "dev")]
    ManualExposure(i32),

    /// No changes will be made to exposure settings except for via debug
    /// commands. This mode is used in dev mode, and also by tests.
    #[cfg(any(feature = "dev", test))]
    FullyManual,
}

impl<S, I, C, T> App<S, I, C, T>
where
    S: spi::Transfer<u8>,
    I: camera::CameraDataInterface,
    C: Classifier,
    T: Timer,
{
    pub fn new(spi: S, camera_data_interface: I, classifier: C, timer: T) -> Self {
        Self {
            mcu: McuInterface::new(spi),
            image_data_receiver: ImageDataReceiver::new(camera_data_interface, timer.clone()),
            classifier,
            #[cfg(feature = "image-transfer")]
            image_transfer_count: 0,
            histogram_enabled: false,
            timer,
            status: Status::default(),
            model_number: 1,
            model_result_count: [0; NUM_MODELS],
            last_model_number: None,
            frame_pending: false,
            trigger_delay_by_model: [INITIAL_FRAME_TRIGGER_DELAY; NUM_MODELS],
            spi_test_data: None,
            test_image_area: None,
            exposure_algorithm: ExposureAlgorithm::MedianTarget(-64),
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
            self.mcu.report_error(mcu_common::Error::TfliteFailure);
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

    pub fn set_test_image_area(&mut self, region: &'static [u8]) {
        self.test_image_area = Some(region);
    }

    pub fn mcu_mut(&mut self) -> &mut McuInterface<S> {
        &mut self.mcu
    }

    fn loop_body(&mut self) -> Result<()> {
        let loop_start_time = self.timer.now();
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

        self.select_model(&features, &config);

        let is_rotated = self.determine_rotation(&config)?;
        let capture_start = self.timer.now();
        let image_metadata = self.capture_well_exposed_image(is_rotated)?;
        let image_wait_ms: Milliseconds = image_metadata.wait_start.into();

        if let Some(last_model_number) = self.last_model_number {
            let model_delay = &mut self.trigger_delay_by_model[last_model_number as usize - 1];
            let adjust = if image_metadata.missed_frame {
                Microseconds(10_000)
            } else if image_metadata.wait_start > Microseconds(15_000) {
                // When we had to wait a long time, move quickly toward a high
                // target.
                Microseconds(12_000) - image_metadata.wait_start
            } else {
                // When we had to wait less time, use a lower target, but move
                // slower towards it.
                (Microseconds(2_500) - image_metadata.wait_start).div(8)
            };
            *model_delay += adjust;
        }

        let capture_ms = self.timer.elapsed_ms(capture_start) - image_wait_ms;
        if self.histogram_enabled {
            let e = self.exposure;
            mcu_println!(
                self.mcu,
                "{} exp: {:#06x}",
                self.image_data_receiver.histogram,
                e
            );
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
        if self.image_data_receiver.histogram.is_usable()
            && (features.contains(Features::MODEL1) || features.contains(Features::MODEL2))
        {
            // Request the next camera frame so that it starts streaming shortly
            // after we finish running our model.
            let frame_delay = self.trigger_delay_by_model[self.model_number as usize - 1];
            self.image_data_receiver.start();
            self.mcu.trigger_frame_in(frame_delay)?;
            self.frame_pending = true;
            self.last_model_number = Some(self.model_number);

            let start = self.timer.now();
            let score = self.classifier.run_model(self.model_number)?;
            let classify_ms = self.timer.elapsed_ms(start);

            let result_count = &mut self.model_result_count[self.model_number as usize - 1];
            *result_count = result_count.wrapping_add(1);
            model_status = 0x8000 | ((score as u16) & 0xff) | ((*result_count & 0x7f) as u16) << 8;
            let total_ms = self.timer.elapsed_ms(loop_start_time);
            mcu_println!(
                self.mcu,
                "Model {} score:{:5} wait:{:4} capture:{:4} model:{:4} other:{:4} total:{:4}",
                self.model_number,
                score,
                image_wait_ms,
                capture_ms,
                classify_ms,
                total_ms - image_wait_ms - capture_ms - classify_ms,
                total_ms,
            );
        } else {
            model_status = 0;
            self.last_model_number = None;
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

        // Set frame rate to ~21Hz by doubling frame length
        camera.set_frame_length(2 * 0x158)?;
        // Set camera to stream
        //camera.set_mode(hm01b0::Mode::Streaming)?;

        match self.exposure_algorithm {
            ExposureAlgorithm::MedianTarget(_) => {
                // Disable AE. We'll control exposure programatically instead.
                camera.set_ae(false)?;
                // Writes the initial exposure configuration
                self.write_exposure()?;
            }
            #[cfg(feature = "dev")]
            ExposureAlgorithm::HardwareAe(target) => {
                camera.set_ae_target(target)?;
            }
            #[cfg(feature = "dev")]
            ExposureAlgorithm::ManualExposure(_) => {
                camera.set_ae(false)?;
            }
            #[cfg(any(feature = "dev", test))]
            ExposureAlgorithm::FullyManual => {
                camera.set_ae(false)?;
            }
        }

        Ok(())
    }

    // Determine what the rotation setting is
    fn determine_rotation(&mut self, config: &application::Configuration) -> Result<bool> {
        Ok(
            Rotation::try_from(config.camera_config & 3).map_err(|_| Error::InvalidArg)?
                == Rotation::Clockwise,
        )
    }

    fn capture_well_exposed_image(&mut self, rotate_clockwise: bool) -> Result<ImageMetadata> {
        match self.exposure_algorithm {
            ExposureAlgorithm::MedianTarget(target) => {
                self.adjust_exposure_and_capture(target as i32, rotate_clockwise)
            }
            #[cfg(feature = "dev")]
            ExposureAlgorithm::ManualExposure(exposure) => {
                self.exposure = exposure;
                self.write_exposure()?;
                self.capture_image(rotate_clockwise)
            }
            #[cfg(any(feature = "dev", test))]
            ExposureAlgorithm::FullyManual => self.capture_image(rotate_clockwise),
            #[cfg(feature = "dev")]
            ExposureAlgorithm::HardwareAe(_) => self.capture_image(rotate_clockwise),
        }
    }

    /// Attempts to converge median to `target` and captures an image.
    fn adjust_exposure_and_capture(
        &mut self,
        target: i32,
        rotate_clockwise: bool,
    ) -> Result<ImageMetadata> {
        // Capture a first image
        let first_metadata = self.capture_image(rotate_clockwise)?;

        const BEGIN_THRESHOLD: i32 = 20;
        const END_THRESHOLD: i32 = 5;
        const MAX_STEPS: usize = 15;
        let mut median = self.image_data_receiver.histogram.median() as i32;
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
                median = self.image_data_receiver.histogram.median() as i32;
                diff = median - target;
                steps += 1;
            }
        }

        // Always take a small step toward the median anyway
        self.step_exposure(if diff < 0 { 16 } else { -16 })?;

        // We only return metadata for the first frame that we captured, since
        // that's the one, if any, that was timed to arrive at a particular
        // time.
        Ok(first_metadata)
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

    fn capture_image(&mut self, rotate_clockwise: bool) -> Result<ImageMetadata> {
        let mut dimensions = camera::ImageDimensions::default();
        if rotate_clockwise {
            dimensions.rotate_90();
        }
        let buffer = self.classifier.input_data_mut(self.model_number);

        let mut missed_frame = false;
        if !self.frame_pending || self.image_data_receiver.has_missed_start_of_frame() {
            // Wait until the camera returns to standby before we request a new
            // frame. Triggering frames when there is still a frame in progress
            // can put the camera into a bad state.
            self.mcu.camera().wait_standby()?;
            self.image_data_receiver.start();
            self.mcu.trigger_frame_in(Microseconds(0))?;
            missed_frame = true;
        }
        let wait_start = self
            .image_data_receiver
            .receive_image_data(buffer, &dimensions)?;

        self.frame_pending = false;

        if rotate_clockwise {
            image_rotation::rotate_240_320_clockwise(buffer);
        }
        Ok(ImageMetadata {
            wait_start,
            missed_frame,
        })
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
                    self.set_exposure_algorithm(ExposureAlgorithm::ManualExposure(arg as i32));
                }
                DebugCommand::SetMedianTarget => {
                    self.set_exposure_algorithm(ExposureAlgorithm::MedianTarget(arg as i8));
                }
                DebugCommand::HardwareAe => {
                    self.set_exposure_algorithm(ExposureAlgorithm::HardwareAe(arg));
                }
                DebugCommand::DisableAutomaticExposure => {
                    self.set_exposure_algorithm(ExposureAlgorithm::FullyManual);
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
        let test_data = self.test_image_area.ok_or_else(|| {
            mcu_println!(self, "Test-image area not available");
            Error::UnknownFeature
        })?;

        mcu_println!(self, "Testing TFLM on canned data");

        const MODEL1_EXPECTED: &[i8] = &include!("../../../test_data/presence.expected");
        const MODEL2_EXPECTED: &[i8] = &include!("../../../test_data/second.expected");

        self.run_model_on_test_images(1, test_data, MODEL1_EXPECTED)?;
        self.run_model_on_test_images(2, test_data, MODEL2_EXPECTED)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    fn run_model_on_test_images(
        &mut self,
        model_number: u32,
        test_data: &[u8],
        expected: &[i8],
    ) -> Result<()> {
        const MAX_RESULTS: usize = TEST_IMAGES_MAX_SIZE as usize / TEST_IMAGE_BLOCK_SIZE;

        let mut results: [i8; MAX_RESULTS] = [0i8; MAX_RESULTS];
        let mut num_results = 0;
        for input_data in test_data.chunks_exact(TEST_IMAGE_BLOCK_SIZE) {
            // Check if the first few bytes of the image look like blank flash.
            // If they do, assume that no image was written here. So long as our
            // test images aren't completely overexposed, this should be
            // suffient.
            if input_data.starts_with(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]) {
                break;
            }

            let buffer = self.classifier.input_data_mut(model_number);
            for i in 0..buffer.len() {
                buffer[i] = (i16::from(input_data[i]) - 128) as i8;
            }

            results[num_results] = self.classifier.run_model(model_number)?;
            num_results += 1;

            // Running all our models on all our data might take several
            // seconds. Let the MCU know that we're still alive after each
            // image.
            self.mcu.resync();
        }
        let results = &results[..num_results];
        if results == expected {
            mcu_println!(self, "Model {model_number} results: {results:?} - PASS");
        } else {
            mcu_println!(self, "Model {model_number} results: {results:?} - FAIL");
            mcu_println!(self, "       expected: {expected:?}");
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
    fn select_model(&mut self, features: &Features, config: &Configuration) {
        // Advance to next model (where the first model is numbered 1).
        self.model_number = (self.model_number % (NUM_MODELS as u32)) + 1;

        // If we're on model 1, but that's not active, advance to model 2.
        if self.model_number == 1 && !features.contains(Features::MODEL1) {
            self.model_number = 2;
        }
        // If our last SPD (model 2) score was over the threshold then only run
        // model 2.
        let spd_threshold = (config.model_repeat_threshold & 0xff) as i8;
        let spd_score = (self.status.second_person_status & 0xff) as i8;
        if spd_threshold != 127 && spd_score >= spd_threshold {
            self.model_number = 2;
        }
        // If our last present (model 1) score was under the presence repeat
        // threshold, then only run model 1.
        let presence_threshold = (config.model_repeat_threshold >> 8 & 0xff) as i8;
        let presence_score = (self.status.person_status & 0xff) as i8;
        if presence_score < presence_threshold {
            self.model_number = 1;
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

    #[cfg(feature = "dev")]
    fn set_exposure_algorithm(&mut self, exposure_algorithm: ExposureAlgorithm) {
        mcu_println!(self, "Exposure algorithm: {:?}", exposure_algorithm);
        self.exposure_algorithm = exposure_algorithm;
        if let Err(error) = self.camera_setup() {
            mcu_println!(self, "Camera setup error: {:?}", error);
        }
    }
}

fn fpga_to_mcu_error(error: Error) -> mcu_common::Error {
    // TODO: Get rid of our own Error enum and just use
    // mcu_common::Error everywhere.
    match error {
        Error::Camera => mcu_common::Error::Camera,
        Error::InvalidArg => mcu_common::Error::HostI2cBadRequest,
        Error::McuTimeout => mcu_common::Error::FpgaMcuCommError,
        Error::SelfTestFailed => mcu_common::Error::SelfTestFailed,
        Error::SpiError => mcu_common::Error::FpgaMcuCommError,
        Error::TfLiteFailure => mcu_common::Error::TfliteFailure,
        Error::UnknownFeature => mcu_common::Error::HostI2cBadRequest,
    }
}

pub fn report_fatal_error(spi: impl spi::Transfer<u8>, error: mcu_common::Error) {
    let mut mcu = McuInterface::new(spi);
    mcu.resync();
    mcu.report_error(error);
}

pub fn report_panic(spi: impl spi::Transfer<u8>, info: &core::panic::PanicInfo) {
    let mut mcu = McuInterface::new(spi);
    mcu.resync();
    mcu.report_panic(info);
}

#[cfg(feature = "dev")]
pub fn write_stdout(spi: impl spi::Transfer<u8>, data: &[u8]) {
    let mut mcu = McuInterface::new(spi);
    // Printing is for debugging purposes, so is best effort only. Ignore
    // errors.
    let _ = mcu.write_stdout_bytes(data);
}

#[cfg(test)]
mod tests {
    use crate::camera::testing::FakeCameraDataInterface;
    use crate::fake_mcu::FakeMcu;
    use crate::timer::testing::FakeTimer;
    use application::Configuration;

    use super::*;

    #[test]
    fn test_model_selection_nomiri() -> Result<()> {
        let mut app = App::new(
            FakeMcu::default(),
            FakeCameraDataInterface::default(),
            FakeClassifier::default(),
            FakeTimer,
        );

        // Turn exposure control off to avoid trying to change exposure of camera fakes.
        app.exposure_algorithm = ExposureAlgorithm::FullyManual;

        // Both models enabled - second person model runs.
        app.classifier.model_outputs[0] = 0x42;
        app.mcu.spi.state.configuration = Configuration {
            enabled_features: (Features::MODEL1 | Features::MODEL2).bits(),
            camera_config: Rotation::Clockwise.into(),
            ..Configuration::default()
        };
        assert_eq!(app.loop_body(), Ok(()));
        let mut expected_status = Status {
            loop_count: 1,
            person_status: 0,
            second_person_status: 0x8142,
            enabled_features: app.mcu.spi.state.configuration.enabled_features,
        };
        assert_eq!(app.mcu.spi.state.status, expected_status);
        assert_eq!(app.classifier.models_executed, 2);

        // Both models enabled - presence detection runs.
        app.classifier.model_outputs[0] = 0x73;
        app.classifier.models_executed = 0;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0x8173;
        assert_eq!(app.mcu.spi.state.status, expected_status);
        assert_eq!(app.classifier.models_executed, 1);
        assert_eq!(app.status.person_status, 0x8173);
        assert_eq!(app.status.second_person_status, 0x8142);

        // SPD over threshold, presence doesn't run.
        app.mcu.spi.state.configuration.model_repeat_threshold = 0xe010;
        app.classifier.model_outputs[0] = 0x63;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.second_person_status = 0x8263;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        app.classifier.model_outputs[0] = 0x53;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.second_person_status = 0x8353;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        // SPD runs, but goes below threshold, triggering presence.
        app.classifier.model_outputs[0] = 0x03;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.second_person_status = 0x8403;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        app.classifier.model_outputs[0] = 0x56;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0x8256;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        app.classifier.model_outputs[0] = 0x04;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.second_person_status = 0x8504;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        // Presence model keeps running while below its threshold (0xe0 or -32).
        app.classifier.model_outputs[0] = -50;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0x83ce;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        app.classifier.model_outputs[0] = -51;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0x84cd;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        app.classifier.model_outputs[0] = 8;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0x8508;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        app.classifier.model_outputs[0] = 6;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.second_person_status = 0x8606;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        // Both models disabled.
        app.classifier.models_executed = 0;
        app.mcu.spi.state.configuration.enabled_features = Features::DISABLED.bits();
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0;
        expected_status.second_person_status = 0;
        expected_status.enabled_features = app.mcu.spi.state.configuration.enabled_features;
        assert_eq!(app.mcu.spi.state.status, expected_status);
        assert_eq!(app.classifier.models_executed, 0);

        Ok(())
    }
}
