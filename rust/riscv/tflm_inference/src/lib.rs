// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

use core::convert::TryFrom;
use fpga_app::Error;
use fpga_app::Result;
use fpga_app::TfLiteInitStatus;

#[link(name = "tflite-micro")]
#[link(name = "c")]
#[link(name = "gcc")]
#[link(name = "m")]
#[link(name = "stdc++")]
extern "C" {
    fn tflite_init(model_bytes: *const u8) -> i32;
    fn tflite_get_input() -> *mut i8;
    fn tflite_classify(score1_out: *mut i8, score2_out: *mut i8) -> bool;
    fn tflite_layer_test() -> bool;
}

const MODEL_SIZE: usize = include_bytes!("../../../../models/shared.tflite").len();

#[link_section = ".model_data"]
static MODEL: [u8; MODEL_SIZE] = *include_bytes!("../../../../models/shared.tflite");

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
            init_status: Self::from_int(tflite_init(MODEL.as_ptr())),
        }
    }

    fn from_int(status: i32) -> TfLiteInitStatus {
        TfLiteInitStatus::try_from(status).unwrap_or(TfLiteInitStatus::OtherFailure)
    }
}

impl fpga_app::Classifier for TflmClassifier {
    fn input_data_mut(&mut self) -> &mut [i8] {
        // safety: TflmClassifier::sole_instance requires that only a single
        // instance of TflmClassifier exists, so we can guarantee that our
        // return value won't alias. The C++ code allocates an arena that is
        // large enough to store NUM_PIXELS.
        unsafe { core::slice::from_raw_parts_mut(tflite_get_input(), fpga_app::camera::NUM_PIXELS) }
    }

    fn input_data(&self) -> &[i8] {
        // safety: TflmClassifier::sole_instance requires that only a single
        // instance of TflmClassifier exists, so we can guarantee that our
        // return value won't alias. The C++ code allocates an arena that is
        // large enough to store NUM_PIXELS.
        unsafe { core::slice::from_raw_parts(tflite_get_input(), fpga_app::camera::NUM_PIXELS) }
    }

    fn run_model(&mut self) -> Result<(i8, i8)> {
        let mut score1 = 0i8;
        let mut score2 = 0i8;
        if unsafe { tflite_classify(&mut score1 as *mut i8, &mut score2 as *mut i8) } {
            Ok((score1, score2))
        } else {
            Err(Error::TfliteFailure)
        }
    }

    fn init_status(&self) -> TfLiteInitStatus {
        self.init_status
    }

    fn layer_test(&mut self) -> Result<()> {
        if unsafe { tflite_layer_test() } == false {
            Err(Error::SelfTestFailed)
        } else {
            Ok(())
        }
    }
}
