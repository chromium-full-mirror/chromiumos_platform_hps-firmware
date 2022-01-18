// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Result;

use num_enum::TryFromPrimitive;

/// Describes how TfLite classifier is initialized
/// Matches values in shim.4
#[derive(Copy, Clone, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
pub enum TfLiteInitStatus {
    InitOk = 0,
    Model1Failed = 1,
    Model2Failed = 2,
    OtherFailure = -1,
}

pub trait Classifier {
    /// Returns a mutable reference into which the input data can be written.
    fn input_data_mut(&mut self, model_number: u32) -> &mut [u8];

    /// Run a model.
    fn run_model(&mut self, model_number: u32) -> Result<i8>;

    /// Did initialization succeed?
    fn init_status(&self) -> TfLiteInitStatus;
}

pub struct FakeClassifier {
    input_data: [u8; crate::camera::NUM_PIXELS],
    /// Which models have been executed. Bit 0 being set means that the first
    /// model has been executed and so on for each successive bit.
    pub(crate) models_executed: u8,
    pub(crate) model_outputs: [i8; 32],
}

impl Classifier for FakeClassifier {
    fn input_data_mut(&mut self, _model_number: u32) -> &mut [u8] {
        self.input_data.as_mut()
    }

    fn run_model(&mut self, model_number: u32) -> Result<i8> {
        self.models_executed |= 1 << (model_number as usize - 1);
        Ok(self.model_outputs[0])
    }
    fn init_status(&self) -> TfLiteInitStatus {
        TfLiteInitStatus::InitOk
    }
}

impl Default for FakeClassifier {
    fn default() -> Self {
        Self {
            input_data: [0u8; crate::camera::NUM_PIXELS],
            models_executed: 0,
            model_outputs: [0i8; 32],
        }
    }
}
