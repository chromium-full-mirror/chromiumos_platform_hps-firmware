// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Context;
use anyhow::Result;
use std::path::PathBuf;

const NUM_PIXELS: usize = 320 * 240;

fn main() -> Result<()> {
    png_to_raw("test_data/not-person-320x240.png", "not-person.raw")?;
    png_to_raw(
        "../../third_party/test_data/Einstein_1921_by_F_Schmutzer_-_restoration.png",
        "person.raw",
    )?;
    Ok(())
}

/// Reads a PNG file from `png_path` and writes the result into the output
/// directory with the name `raw_filename`. Input PNG must be 320x240 grayscale.
fn png_to_raw(png_path: &str, raw_filename: &str) -> Result<()> {
    use image::ImageDecoder;
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let mut pixels = vec![0; NUM_PIXELS];
    let png_bytes = std::fs::read(png_path)?;
    image::codecs::png::PngDecoder::new(png_bytes.as_slice())?.read_image(&mut pixels)?;
    convert_pixels_to_signed(&mut pixels);
    let raw_filename = out_dir.join(raw_filename);
    std::fs::write(&raw_filename, pixels)
        .with_context(|| format!("Failed to write {}", raw_filename.display()))?;
    Ok(())
}

/// Converts pixel bytes to be in the form expected by the ML model (signed
/// bytes).
fn convert_pixels_to_signed(pixels: &mut [u8]) {
    for pixel in pixels {
        *pixel = pixel.wrapping_sub(128);
    }
}
