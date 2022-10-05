// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Result;

use num_enum::TryFromPrimitive;

/// Describes how TfLite classifier is initialized
/// Matches values in shim.h
#[derive(Copy, Clone, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
pub enum TfLiteInitStatus {
    InitOk = 0,
    Model1Failed = 1,
    Model2Failed = 2,
    CfuBug = 3,
    OtherFailure = -1,
}

pub trait Classifier {
    /// Returns a mutable reference into which the input data can be written.
    fn input_data_mut(&mut self) -> &mut [i8];

    /// Returns a non-mutable reference to the data already written.
    fn input_data(&self) -> &[i8];

    /// Run the model.
    fn run_model(&mut self) -> Result<(i8, i8)>;

    /// Did initialization succeed?
    fn init_status(&self) -> TfLiteInitStatus;

    // Self-test for classifier.
    fn layer_test(&mut self) -> Result<()>;
}

pub struct FakeClassifier {
    input_data: [i8; crate::camera::NUM_PIXELS],
    pub(crate) model_executed: bool,
    pub(crate) model_outputs: [i8; 32],
}

impl Classifier for FakeClassifier {
    fn input_data_mut(&mut self) -> &mut [i8] {
        self.input_data.as_mut()
    }

    fn input_data(&self) -> &[i8] {
        &self.input_data
    }

    fn run_model(&mut self) -> Result<(i8, i8)> {
        self.model_executed = true;
        Ok((self.model_outputs[0], self.model_outputs[1]))
    }
    fn init_status(&self) -> TfLiteInitStatus {
        TfLiteInitStatus::InitOk
    }
    fn layer_test(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Default for FakeClassifier {
    fn default() -> Self {
        Self {
            input_data: [0i8; crate::camera::NUM_PIXELS],
            model_executed: false,
            model_outputs: [0i8; 32],
        }
    }
}
