// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub trait Classifier {
    /// Returns a mutable reference into which the input data can be written.
    fn input_data_mut(&mut self) -> &mut [u8];

    /// Classify the data previously written into the buffer returned by
    /// `input_data_mut`
    fn classify_presence(&mut self) -> i8;
}

pub struct FakeClassifier {
    input_data: [u8; crate::camera::NUM_PIXELS],
}

impl Classifier for FakeClassifier {
    fn input_data_mut(&mut self) -> &mut [u8] {
        self.input_data.as_mut()
    }

    fn classify_presence(&mut self) -> i8 {
        0
    }
}

impl Default for FakeClassifier {
    fn default() -> Self {
        Self {
            input_data: [0u8; crate::camera::NUM_PIXELS],
        }
    }
}
