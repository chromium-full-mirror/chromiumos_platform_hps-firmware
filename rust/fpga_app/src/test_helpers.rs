// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::camera::NUM_PIXELS;

// Retrieves i8 pixel values from a PNG image
pub fn png_pixels(png_bytes: &[u8]) -> image::ImageResult<Vec<i8>> {
    use image::ImageDecoder;
    let mut raw = vec![0; NUM_PIXELS];
    image::codecs::png::PngDecoder::new(png_bytes)?.read_image(&mut raw)?;
    let result = raw.iter().map(|&v| ((v as i16) - 128) as i8).collect();
    Ok(result)
}
