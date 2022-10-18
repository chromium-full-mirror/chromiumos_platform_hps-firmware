// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::camera::NUM_PIXELS;

pub mod images {
    pub const WHITE: &[u8] = include_bytes!("../test_data/unusable-320x240.png");
    pub const NOT_PERSON: &[u8] = include_bytes!("../test_data/not-person-320x240.png");
}

// Retrieves i8 pixel values from a PNG image
pub fn png_pixels(png_bytes: &[u8]) -> image::ImageResult<Vec<i8>> {
    use image::ImageDecoder;
    let mut raw = vec![0; NUM_PIXELS];
    image::codecs::png::PngDecoder::new(png_bytes)?.read_image(&mut raw)?;
    let result = raw.iter().map(|&v| ((v as i16) - 128) as i8).collect();
    Ok(result)
}
