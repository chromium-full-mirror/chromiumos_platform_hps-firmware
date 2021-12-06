// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Result;

pub const IMAGE_WIDTH: u32 = 320;
pub const IMAGE_HEIGHT: u32 = 240;
pub const NUM_PIXELS: usize = IMAGE_WIDTH as usize * IMAGE_HEIGHT as usize;

// These constants are randomly generated to reduce the chances of them
// appearing by chance in actual image data. They are duplicated in the C code
// that sends the images.
pub const START_IMAGE_MARKER: [u8; 8] = [13, 174, 212, 250, 191, 30, 138, 125];
pub const END_IMAGE_MARKER: [u8; 8] = [227, 114, 212, 105, 101, 111, 21, 193];

pub const AE_TARGET: u8 = 100;

/// If the AE_MEAN is more than this threshold away from the AE_TARGET for
/// several frames, then we'll recalibrate.
pub const AE_RECALIBRATION_THRESHOLD: i16 = 20;

pub trait ImageDataReceiver {
    /// Configures camera DMA to receive into `buffer` then calls
    /// `trigger_callback` then waits for DMA to complete. If `trigger_callback`
    /// returns an error, then DMA is cancelled and the error is returned.
    fn receive_image_data(
        &mut self,
        buffer: &mut [u8],
        image_dimensions: &ImageDimensions,
        trigger_callback: impl FnOnce() -> Result<()>,
    ) -> Result<()>;
}

pub struct ImageDimensions {
    pub column_start: u32,
    pub column_count: u32,
    pub row_start: u32,
    pub row_count: u32,
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) struct FakeImageDataReceiver;

    impl ImageDataReceiver for FakeImageDataReceiver {
        fn receive_image_data(
            &mut self,
            _buffer: &mut [u8],
            _image_dimensions: &ImageDimensions,
            trigger_callback: impl FnOnce() -> Result<()>,
        ) -> Result<()> {
            (trigger_callback)()
        }
    }
}
