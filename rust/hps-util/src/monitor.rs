// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use fpga_app::Features;
use hps_interface::Hps;
use mcu_common::registers::Register;
use std::fmt::Display;
use std::io::Write;
use std::time::Instant;

pub fn run(hps: &mut dyn Hps, use_interrupt: bool) -> Result<()> {
    hps.launch_app()?;

    // Configure enabled features.
    let enabled_features = Features::MODEL1 | Features::MODEL2;
    hps.write_register(Register::EnabledFeatures, enabled_features.bits())?;

    let mut last_loop_counter = 0;

    let mut prev_model1 = ModelOutput::default();
    let mut prev_model2 = ModelOutput::default();
    let mut has_model1_output = false;
    let mut latencies = Vec::new();

    loop {
        // Wait for new scores to be ready.
        let start = Instant::now();
        if use_interrupt {
            hps.wait_for_interrupt()?;
        } else {
            // Busy-wait until the FPGA loop counter increments, indicating that there should be
            // a score update.
            loop {
                let loop_counter = hps.read_register(Register::FpgaLoopCount)?;
                if loop_counter != last_loop_counter {
                    last_loop_counter = loop_counter;
                    break;
                }
            }
        }
        latencies.push(start.elapsed().as_millis());

        let model1 = ModelOutput::from_u16(hps.read_register(Register::UserPresentStatus)?);
        let model2 = ModelOutput::from_u16(hps.read_register(Register::SecondPersonStatus)?);

        if model1 != prev_model1 {
            prev_model1 = model1;
            if has_model1_output {
                // Print out the S1 latencies for the row we're finishing, which
                // doesn't include the most recent latency.
                let last_latency = latencies.pop().unwrap();
                println!("                 latencies={latencies:?}");
                latencies.clear();
                latencies.push(last_latency);
            }
            print!("S1: {:>4}", model1.to_string());
            std::io::stdout().flush()?;
            has_model1_output = true;
        }
        if model2 != prev_model2 {
            prev_model2 = model2;
            if !has_model1_output {
                print!("        ");
            }
            println!(
                "      S2: {:>4}   latencies={latencies:?}",
                model2.to_string()
            );
            latencies.clear();
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
