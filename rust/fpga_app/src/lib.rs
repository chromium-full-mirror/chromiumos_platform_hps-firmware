// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

pub mod camera;
mod classifier;
mod debug_commands;
mod debug_led;
mod features;
mod mcu_interface;

pub use classifier::Classifier;
pub use classifier::FakeClassifier;
use core::fmt::Write;
pub use debug_commands::DebugCommand;
pub use debug_led::DebugLed;
use embedded_hal::blocking::spi;
use features::Features;
use mcu_interface::McuInterface;

const SOC_ROM_VERSION: u8 = 1;

/// Print to hps-mon via the MCU. The first argument can be anything that
/// implements `mcu_mut()`.
macro_rules! mcu_println {
    ($mcu:expr, $($tts:tt)*) => {
        let _ = writeln!($mcu.mcu_mut(), $($tts)*);
    };
}

#[derive(Debug)]
pub enum Error {
    SpiError,
    McuTimeout,
    InvalidArg,
    UnknownFeature,
}

pub type Result<T> = core::result::Result<T, Error>;

pub struct App<S, I, L, C> {
    mcu: McuInterface<S>,
    image_data_receiver: I,
    classifier: C,
    led: L,
    #[cfg(feature = "image-transfer")]
    image_transfer_enabled: bool,
    classification_enabled: bool,
}

impl<S, I, L, C> App<S, I, L, C>
where
    S: spi::Transfer<u8>,
    I: camera::ImageDataReceiver,
    L: DebugLed,
    C: Classifier,
{
    pub fn new(spi: S, image_data_receiver: I, classifier: C, led: L) -> Self {
        Self {
            mcu: McuInterface::new(spi),
            image_data_receiver,
            classifier,
            led,
            #[cfg(feature = "image-transfer")]
            image_transfer_enabled: false,
            classification_enabled: false,
        }
    }

    pub fn run(&mut self) -> ! {
        self.mcu.resync();
        let _ = self.mcu.report_boot(SOC_ROM_VERSION);
        mcu_println!(self, "Hello from the Rust FPGA");
        self.led.turn_on();
        loop {
            if let Err(error) = self.loop_body() {
                let _ = writeln!(&mut self.mcu, "Error: {:?}", error);
            }
        }
    }

    pub fn mcu_mut(&mut self) -> &mut McuInterface<S> {
        &mut self.mcu
    }

    fn loop_body(&mut self) -> Result<()> {
        let config = self.mcu.read_config()?;
        let features = Features::from_bits(config.enabled_features).ok_or(Error::UnknownFeature)?;

        self.process_debug_commands();

        if features.contains(Features::DEBUG_LED) {
            self.led.turn_on();
        } else {
            self.led.turn_off();
        }

        self.capture_image()?;

        if features.contains(Features::CLASSIFY_PRESENCE) || self.classification_enabled {
            let score = self.classifier.classify_presence();
            mcu_println!(self, "Score: {}", score);
        }

        // Image transfer can be enabled either via a feature (transfer over
        // I2C) or via a hps-mon debug command (transfer over RTT).
        #[cfg(feature = "image-transfer")]
        {
            if features.contains(Features::IMAGE_TRANSFER) || self.image_transfer_enabled {
                self.mcu.send_image_data(self.classifier.input_data_mut())?;
            }
        }

        Ok(())
    }

    fn capture_image(&mut self) -> Result<()> {
        let image_dimensions = camera::ImageDimensions {
            column_start: 2,
            column_count: camera::IMAGE_WIDTH,
            row_start: (324 - camera::IMAGE_HEIGHT) / 2,
            row_count: camera::IMAGE_HEIGHT,
        };
        mcu_println!(self, "Capturing image");
        let mcu = &mut self.mcu;
        mcu.wait_for_camera_standby();
        self.image_data_receiver.receive_image_data(
            self.classifier.input_data_mut(),
            &image_dimensions,
            || mcu.trigger_frame(),
        )?;
        Ok(())
    }

    fn process_debug_commands(&mut self) {
        while let Some((command, arg)) = self.mcu.next_debug_command() {
            match command {
                DebugCommand::ClassificationOn => {
                    self.classification_enabled = true;
                    mcu_println!(self, "Classification enabled");
                }
                DebugCommand::ClassificationOff => {
                    self.classification_enabled = false;
                    mcu_println!(self, "Classification disabled");
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
                _ => {
                    mcu_println!(self, "DebugCommand::{:?}({}) not implemented", command, arg);
                }
            }
        }
    }
}
