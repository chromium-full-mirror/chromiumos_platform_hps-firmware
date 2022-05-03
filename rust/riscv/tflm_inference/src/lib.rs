// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

use fpga_app::Error::TfLiteFailure;
use fpga_app::Result;
use fpga_app::TfLiteInitStatus;

use core::convert::TryFrom;

#[link(name = "tflite-micro")]
#[link(name = "c")]
#[link(name = "gcc")]
#[link(name = "nosys")]
#[link(name = "m")]
#[link(name = "stdc++")]
extern "C" {
    fn tflite_init(model1_bytes: *const u8, model2_bytes: *const u8) -> i32;
    fn tflite_get_input(model_number: u32) -> *mut i8;
    fn tflite_classify(model_number: u32, score_out: *mut i8) -> bool;
}

const MODEL1_SIZE: usize = include_bytes!("../../../../models/presence.tflite").len();
const MODEL2_SIZE: usize = include_bytes!("../../../../models/second.tflite").len();

#[link_section = ".model_data"]
static MODEL1: [u8; MODEL1_SIZE] = *include_bytes!("../../../../models/presence.tflite");
#[link_section = ".model_data"]
static MODEL2: [u8; MODEL2_SIZE] = *include_bytes!("../../../../models/second.tflite");

#[non_exhaustive]
pub struct TflmClassifier {
    init_status: TfLiteInitStatus,
}

impl TflmClassifier {
    /// # Safety
    ///
    /// Must only be called once.
    pub unsafe fn sole_instance() -> Self {
        Self {
            init_status: Self::from_int(tflite_init(MODEL1.as_ptr(), MODEL2.as_ptr())),
        }
    }

    fn from_int(status: i32) -> TfLiteInitStatus {
        TfLiteInitStatus::try_from(status).unwrap_or(TfLiteInitStatus::OtherFailure)
    }
}

impl fpga_app::Classifier for TflmClassifier {
    fn input_data_mut(&mut self, model_number: u32) -> &mut [i8] {
        // safety: TflmClassifier::sole_instance requires that only a single
        // instance of TflmClassifier exists, so we can guarantee that our
        // return value won't alias. The C++ code allocates an arena that is
        // large enough to store NUM_PIXELS.
        unsafe {
            core::slice::from_raw_parts_mut(
                tflite_get_input(model_number),
                fpga_app::camera::NUM_PIXELS,
            )
        }
    }

    fn input_data(&self, model_number: u32) -> &[i8] {
        // safety: TflmClassifier::sole_instance requires that only a single
        // instance of TflmClassifier exists, so we can guarantee that our
        // return value won't alias. The C++ code allocates an arena that is
        // large enough to store NUM_PIXELS.
        unsafe {
            core::slice::from_raw_parts(
                tflite_get_input(model_number),
                fpga_app::camera::NUM_PIXELS,
            )
        }
    }

    fn run_model(&mut self, model_number: u32) -> Result<i8> {
        let mut score = 0i8;
        if unsafe { tflite_classify(model_number, &mut score as *mut i8) } {
            Ok(score)
        } else {
            Err(TfLiteFailure)
        }
    }

    fn init_status(&self) -> TfLiteInitStatus {
        self.init_status
    }
}
