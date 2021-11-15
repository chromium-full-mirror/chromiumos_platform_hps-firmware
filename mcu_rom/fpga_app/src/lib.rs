// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

mod debug_commands;
mod debug_led;
mod features;
mod mcu_interface;

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
    UnknownFeature,
}

type Result<T> = core::result::Result<T, Error>;

pub struct App<S, L>
where
    L: DebugLed,
{
    mcu: McuInterface<S>,
    led: L,
}

impl<S, L> App<S, L>
where
    S: spi::Transfer<u8>,
    L: DebugLed,
{
    pub fn new(spi: S, led: L) -> Self {
        Self {
            mcu: McuInterface::new(spi),
            led,
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

        Ok(())
    }

    fn process_debug_commands(&mut self) {
        while let Some((command, arg)) = self.mcu.next_debug_command() {
            mcu_println!(self, "DebugCommand::{:?}({}) not implemented", command, arg);
        }
    }
}
