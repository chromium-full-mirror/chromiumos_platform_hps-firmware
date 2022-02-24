// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use fpga_app::Features;
use hps_interface::Hps;
use mcu_common::registers;
use std::fmt::Display;
use std::io::Write;
use std::time::Duration;

pub fn run(hps: &mut dyn Hps) -> Result<()> {
    hps.launch_app()?;

    // Configure enabled features.
    let enabled_features = Features::MODEL1 | Features::MODEL2;
    hps.write_register(registers::ENABLED_FEATURES, enabled_features.bits())?;
    hps.write_register(registers::MODEL_REPEAT_THRESHOLD, 0xe520)?;

    let mut last_loop_counter = 0;

    let mut prev_model1 = ModelOutput::default();
    let mut prev_model2 = ModelOutput::default();
    let mut has_model1_output = false;

    loop {
        // Wait until the FPGA completes a loop.
        loop {
            let loop_counter = hps.read_register(registers::FPGA_LOOP_COUNT)?;
            if loop_counter != last_loop_counter {
                last_loop_counter = loop_counter;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        let model1 = ModelOutput::from_u16(hps.read_register(registers::USER_PRESENT_STATUS)?);
        let model2 = ModelOutput::from_u16(hps.read_register(registers::SECOND_PERSON_STATUS)?);

        if model1 != prev_model1 {
            prev_model1 = model1;
            if has_model1_output {
                println!();
            }
            print!("S1: {:10}", model1.to_string());
            std::io::stdout().flush()?;
            has_model1_output = true;
        }
        if model2 != prev_model2 {
            prev_model2 = model2;
            if !has_model1_output {
                print!("              ");
            }
            println!("S2: {:10}", model2.to_string());
            has_model1_output = false;
        }
    }
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
struct ModelOutput {
    usable: bool,
    counter: u8,
    score: i8,
}

impl ModelOutput {
    fn from_u16(value: u16) -> Self {
        Self {
            usable: (value & 0x8000) != 0,
            counter: ((value >> 8) & 0x7f) as u8,
            score: (value & 0xff) as i8,
        }
    }
}

impl Display for ModelOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.usable {
            write!(f, "{}", self.score)
        } else {
            write!(f, "Unusable")
        }
    }
}
