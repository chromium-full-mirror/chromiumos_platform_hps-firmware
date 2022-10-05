// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use fpga_app::camera::NUM_PIXELS;
use fpga_app::Classifier;
use fpga_app::Error;
use fpga_app::Result;
use fpga_app::TfLiteInitStatus;

#[link_section = ".arena"]
static mut IMAGE_DATA: [i8; NUM_PIXELS] = [0i8; NUM_PIXELS];

pub(crate) struct NonTflmClassifier {
    data: &'static mut [i8],
}

impl NonTflmClassifier {
    /// # Safety:
    /// Only one instance may exist at a time.
    pub(crate) unsafe fn sole_instance() -> Self {
        Self {
            data: &mut IMAGE_DATA,
        }
    }
}

impl Classifier for NonTflmClassifier {
    fn input_data_mut(&mut self) -> &mut [i8] {
        self.data
    }

    fn input_data(&self) -> &[i8] {
        self.data
    }

    fn run_model(&mut self) -> Result<(i8, i8)> {
        Err(Error::TfliteFailure)
    }

    fn init_status(&self) -> TfLiteInitStatus {
        TfLiteInitStatus::InitOk
    }

    fn layer_test(&mut self) -> Result<()> {
        Ok(())
    }
}
