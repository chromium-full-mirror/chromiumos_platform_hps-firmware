// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

#[link(name = "tensorflow-microlite")]
#[link(name = "c")]
#[link(name = "gcc")]
#[link(name = "nosys")]
#[link(name = "m")]
#[link(name = "stdc++")]
extern "C" {
    fn tflite_init(error_callback: unsafe extern "C" fn(*const u8));
    fn tflite_get_input() -> *mut u8;
    fn tflite_classify(score_out: *mut i8);
}

unsafe extern "C" fn report_error(_format: *const u8) {
    // TODO: Implement error reporting. We'll need to change the code in
    // shim.cc.
}

#[non_exhaustive]
pub struct TflmClassifier {}

impl TflmClassifier {
    /// # Safety
    ///
    /// Must only be called once.
    pub unsafe fn sole_instance() -> Self {
        tflite_init(report_error);
        Self {}
    }
}

impl fpga_app::Classifier for TflmClassifier {
    fn input_data_mut(&mut self) -> &mut [u8] {
        // safety: TflmClassifier::sole_instance requries that only a single
        // instance of TflmClassifier exists, so we can guarantee that our
        // return value won't alias. The C++ code allocates an arena that is
        // large enough to store NUM_PIXELS.
        unsafe { core::slice::from_raw_parts_mut(tflite_get_input(), fpga_app::camera::NUM_PIXELS) }
    }

    fn classify_presence(&mut self) -> i8 {
        let mut score = 0i8;
        unsafe { tflite_classify(&mut score as *mut i8) };
        score
    }
}
