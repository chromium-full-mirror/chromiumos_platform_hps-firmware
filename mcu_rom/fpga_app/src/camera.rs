// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Result;

pub const IMAGE_WIDTH: u32 = 320;
pub const IMAGE_HEIGHT: u32 = 240;
pub const NUM_PIXELS: usize = IMAGE_WIDTH as usize * IMAGE_HEIGHT as usize;

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
