// Copyright 2021 The ChromiumOS Authors
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
use log::info;
pub use mcu_common::Error;
use mcu_common::SPI_BLOCK_SIZE;
pub use mcu_interface::McuInterface;
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
    model_result_count: [u8; NUM_MODELS],
    /// Whether we've triggered a frame that we're still waiting to receive.
    frame_pending: bool,
    /// How long we'll wait before triggering a camera frame when we're about to
    /// run our model.
    trigger_delay: Microseconds,
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
            model_result_count: [0; NUM_MODELS],
            frame_pending: false,
            trigger_delay: INITIAL_FRAME_TRIGGER_DELAY,
            spi_test_data: None,
            test_image_area: None,
            exposure_algorithm: ExposureAlgorithm::MedianTarget(-64),
            exposure: 0x100,
        }
    }

    pub fn run(&mut self) -> ! {
        self.mcu.resync();
        let _ = self.mcu.report_boot(SOC_ROM_VERSION);
        info!("Hello from the Rust FPGA");
        let status = self.classifier.init_status();
        info!("Classifier status: {:?}", status);
        if status != TfLiteInitStatus::InitOk {
            self.mcu.report_error(Error::TfliteFailure);
        }
        if let Err(error) = self.classifier.layer_test() {
            info!("Classifier self-test error: {:?}", error);
            self.mcu.report_error(error);
        }
        if let Err(error) = self.camera_setup() {
            info!("Camera error: {:?}", error);
            self.mcu.report_error(error);
        }
        loop {
            if let Err(error) = self.loop_body() {
                info!("Error: {:?}", error);
                self.mcu.report_error(error);
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
        let config = self.mcu.read_config()?;
        let features =
            Features::from_bits(config.enabled_features).ok_or(Error::HostI2cBadRequest)?;
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
        if features.contains(Features::PANIC) {
            panic!("Intentional FPGA panic");
        }

        #[cfg(feature = "dev")]
        {
            self.process_debug_commands();
        }

        let is_rotated = self.determine_rotation(&config)?;
        let capture_start = self.timer.now();
        let image_metadata = self.capture_well_exposed_image(is_rotated)?;
        let image_wait_ms: Milliseconds = image_metadata.wait_start.into();

        let model_delay = &mut self.trigger_delay;
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
        if *model_delay < Microseconds(0) {
            *model_delay = Microseconds(0)
        }

        let capture_ms = self.timer.elapsed_ms(capture_start) - image_wait_ms;
        if self.histogram_enabled {
            let e = self.exposure;
            info!("{} exp: {:#06x}", self.image_data_receiver.histogram, e);
        }

        // Image transfer can be enabled either via a feature (transfer over
        // I2C) or via a hps-mon debug command (transfer over RTT).
        #[cfg(feature = "image-transfer")]
        {
            if features.contains(Features::IMAGE_TRANSFER) || self.image_transfer_count != 0 {
                self.mcu.send_image_data(self.classifier.input_data())?;
            }
            if self.image_transfer_count != 0 && self.image_transfer_count != u32::MAX {
                self.image_transfer_count -= 1;
            }
        }

        // Classification needs to be done after any other uses of the image
        // (e.g. image transfer), since it overwrites the image buffer with
        // other data.
        if self.image_data_receiver.histogram.is_usable()
            && (features.contains(Features::MODEL1) || features.contains(Features::MODEL2))
        {
            // Request the next camera frame so that it starts streaming shortly
            // after we finish running our model.
            self.image_data_receiver.start();
            self.mcu.trigger_frame_in(self.trigger_delay)?;
            self.frame_pending = true;

            let scores = self.classifier.run_model()?;

            if features.contains(Features::MODEL1) {
                let result_count = &mut self.model_result_count[0];
                *result_count = result_count.wrapping_add(1);
                self.status.person_status =
                    0x8000 | ((scores.0 as u16) & 0xff) | ((*result_count & 0x7f) as u16) << 8;
            }
            if features.contains(Features::MODEL2) {
                let result_count = &mut self.model_result_count[1];
                *result_count = result_count.wrapping_add(1);
                self.status.second_person_status =
                    0x8000 | ((scores.1 as u16) & 0xff) | ((*result_count & 0x7f) as u16) << 8;
            }
            info!(
                "scores:{scores:4?} wait:{:4} capture:{:4}",
                image_wait_ms, capture_ms,
            );
        }

        self.status.loop_count = self.status.loop_count.wrapping_add(1);
        self.status.enabled_features = config.enabled_features;
        self.mcu.report_status(&self.status)?;

        Ok(())
    }

    fn camera_setup(&mut self) -> Result<()> {
        let mut camera = self.mcu.camera();

        // Set frame rate to ~21Hz by doubling frame length
        camera.set_frame_length(2 * 0x158)?;

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
            Rotation::try_from(config.camera_config & 3).map_err(|_| Error::HostI2cBadRequest)?
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
        let buffer = self.classifier.input_data_mut();

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
                    info!("Image transfer: {}", arg != 0);
                }
                #[cfg(feature = "image-transfer")]
                DebugCommand::TransferCount => {
                    self.image_transfer_count = arg as u32;
                    info!("Image transfer enabled ({} images)", arg);
                }
                DebugCommand::Histogram => {
                    self.histogram_enabled = arg != 0;
                    info!("Histogram: {}", arg != 0);
                }
                DebugCommand::SelfTest => {
                    if self.self_test().is_err() {
                        info!("Self-test failed");
                    }
                }
                DebugCommand::TestSpiFlashReads => {
                    info!("Performing SPI flash read integrity test...");
                    self.spi_flash_read_test();
                }
                DebugCommand::TestFpgaMcuComms => {
                    info!("Performing FPGA<->MCU communication test...");
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
                    info!("DebugCommand::{:?}({}) not implemented", command, arg);
                }
            }
        }
    }

    #[cfg(feature = "dev")]
    fn self_test(&mut self) -> Result<()> {
        if !cfg!(feature = "self-test") {
            info!("Self-test feature not enabled");
            return Err(Error::HostI2cBadRequest);
        }
        let test_data = self.test_image_area.ok_or_else(|| {
            info!("Test-image area not available");
            Error::HostI2cBadRequest
        })?;

        info!("Testing TFLM on canned data");

        const MAX_RESULTS: usize = TEST_IMAGES_MAX_SIZE as usize / TEST_IMAGE_BLOCK_SIZE;

        let mut presence_results: [i8; MAX_RESULTS] = [0; MAX_RESULTS];
        let mut second_person_results: [i8; MAX_RESULTS] = [0; MAX_RESULTS];
        let mut num_results = 0;
        for input_data in test_data.chunks_exact(TEST_IMAGE_BLOCK_SIZE) {
            // Check if the first few bytes of the image look like blank flash.
            // If they do, assume that no image was written here. So long as our
            // test images aren't completely overexposed, this should be
            // suffient.
            if input_data.starts_with(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]) {
                break;
            }

            let buffer = self.classifier.input_data_mut();
            for i in 0..buffer.len() {
                buffer[i] = (i16::from(input_data[i]) - 128) as i8;
            }

            let outputs = self.classifier.run_model()?;
            presence_results[num_results] = outputs.0;
            second_person_results[num_results] = outputs.1;
            num_results += 1;

            // Running all our models on all our data might take several
            // seconds. Let the MCU know that we're still alive after each
            // image.
            self.mcu.resync();
        }

        self.check_results(
            "Presence",
            num_results,
            &presence_results,
            &include!("../../../test_data/presence.expected"),
        );
        self.check_results(
            "Second person",
            num_results,
            &second_person_results,
            &include!("../../../test_data/second.expected"),
        );

        Ok(())
    }

    #[cfg(feature = "dev")]
    fn check_results(&mut self, name: &str, num_results: usize, results: &[i8], expected: &[i8]) {
        let results = &results[..num_results];
        if results == expected {
            info!("{name:>13} results: {results:?} - PASS");
        } else {
            info!("{name:>13} results: {results:?} - FAIL");
            info!("             expected: {expected:?}");
        }
    }

    /// Repeatedly reads some test data that we've previously written to the SPI
    /// flash and checks it against the expected pattern, reporting the number
    /// of iteration as well as the number of bad reads we've encountered.
    fn spi_flash_read_test(&mut self) {
        let test_data = match self.spi_test_data {
            Some(x) => x,
            None => {
                info!("SPI test region not supplied");
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
                info!(
                    "{} iterations with {} errors so far",
                    self.status.loop_count, bad
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
            info!(
                "{} iterations with {} errors so far",
                self.status.loop_count, bad
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
        camera_tester::check_test_pattern(self.classifier.input_data_mut())?;
        Ok(())
    }

    /// Writes exposure settings to the camera, assuming they are set
    pub fn write_exposure(&mut self) -> Result<()> {
        let e = camera::calculate_exposure(self.exposure);
        Ok(self.mcu.camera().set_exposure(&e)?)
    }

    #[cfg(feature = "dev")]
    fn set_exposure_algorithm(&mut self, exposure_algorithm: ExposureAlgorithm) {
        info!("Exposure algorithm: {:?}", exposure_algorithm);
        self.exposure_algorithm = exposure_algorithm;
        if let Err(error) = self.camera_setup() {
            info!("Camera setup error: {:?}", error);
        }
    }
}

pub fn report_fatal_error(spi: impl spi::Transfer<u8>, error: Error) {
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
    fn test_feature_toggle_nomiri() -> Result<()> {
        let mut app = App::new(
            FakeMcu::default(),
            FakeCameraDataInterface::default(),
            FakeClassifier::default(),
            FakeTimer,
        );

        // Turn exposure control off to avoid trying to change exposure of camera fakes.
        app.exposure_algorithm = ExposureAlgorithm::FullyManual;

        // Iteration: Both features enabled.
        app.classifier.model_outputs[0] = 0x42;
        app.classifier.model_outputs[1] = 0x12;
        app.mcu.spi.state.configuration = Configuration {
            enabled_features: (Features::MODEL1 | Features::MODEL2).bits(),
            camera_config: Rotation::Clockwise.into(),
        };
        assert_eq!(app.loop_body(), Ok(()));
        let mut expected_status = Status {
            loop_count: 1,
            person_status: 0x8142,
            second_person_status: 0x8112,
            enabled_features: app.mcu.spi.state.configuration.enabled_features,
        };
        assert_eq!(app.mcu.spi.state.status, expected_status);
        assert!(app.classifier.model_executed);

        // Iteration 2: Both features enabled.
        app.classifier.model_outputs[0] = 0x73;
        app.classifier.model_outputs[1] = 0x24;
        app.classifier.model_executed = false;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0x8273;
        expected_status.second_person_status = 0x8224;
        assert_eq!(app.mcu.spi.state.status, expected_status);
        assert!(app.classifier.model_executed);
        assert_eq!(app.status.person_status, 0x8273);
        assert_eq!(app.status.second_person_status, 0x8224);

        // Iteration 3: Only presence enabled.
        app.mcu.spi.state.configuration.enabled_features = Features::MODEL1.bits();
        app.classifier.model_outputs[0] = 0x52;
        app.classifier.model_outputs[1] = 0x59;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.enabled_features = Features::MODEL1.bits();
        expected_status.person_status = 0x8352;
        expected_status.second_person_status = 0;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        // Iteration 4: Only SPD enabled.
        app.mcu.spi.state.configuration.enabled_features = Features::MODEL2.bits();
        app.classifier.model_outputs[0] = 0x55;
        app.classifier.model_outputs[1] = 0x57;
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.enabled_features = Features::MODEL2.bits();
        expected_status.person_status = 0;
        expected_status.second_person_status = 0x8357;
        assert_eq!(app.mcu.spi.state.status, expected_status);

        // Both features disabled.
        app.classifier.model_executed = false;
        app.mcu.spi.state.configuration.enabled_features = Features::DISABLED.bits();
        assert_eq!(app.loop_body(), Ok(()));
        expected_status.loop_count += 1;
        expected_status.person_status = 0;
        expected_status.second_person_status = 0;
        expected_status.enabled_features = app.mcu.spi.state.configuration.enabled_features;
        assert_eq!(app.mcu.spi.state.status, expected_status);
        assert!(!app.classifier.model_executed);

        Ok(())
    }
}
