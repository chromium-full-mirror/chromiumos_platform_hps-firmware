// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Images with entropy below the threshold are usable.
// The entropy threshold was defined empirically.
// For more details, please refer to b/204518775#comment9.
pub const ENTROPY_THRESHOLD: u32 = 270000000;

// Calculates an approximation of ln(x)/ln(2) * 2^24.
//
// Accurately calculates the integer portion of ln(x)/ln(2), then relies on
// the approximation ln(x) ~= (x-1) to calculate the fractional part of the base
// 2 logarithm. Bit shifting is used to pack integer and fraction into a 32 bit
// result that is a fixed point number, with the integer part in the top 8 bits
// and fractional part in the lower 24 bits.
// The resulting output is accurate to within 6%, with error (generally)
// decreasing for larger x."
fn fixed_log2(x: u64) -> u64 {
    let shift: u64 = 24;
    let mut exp: u64 = 0;
    while x >= (1 << (exp + 1)) {
        exp += 1;
    }
    let frac: u64 = x - (1 << exp);
    (exp << shift) + (frac << (shift - exp))
}

// This function calculates if the most recently taken image is usable.
// Images which contain most of the pixels in a small number of buckets
// will have much greater entropy value.
pub fn calculate_entropy(image_data: &mut [u8]) -> u32 {
    let mut histogram: [u32; 256] = [0; 256];
    for i in image_data.iter() {
        histogram[*i as usize] += 1;
    }
    // This method of calculating entropy is very custom and it has been
    // achieved and tested using Rust adequate.
    // For more details, please refer to b/204518775#comment9.
    let mut entropy: u64 = 0;
    for value in histogram {
        if value == 0 {
            continue;
        }
        // To preserve some of the fraction value, smaller shift is applied.
        entropy += (value as u64 * fixed_log2(value as u64)) >> 16;
    }
    entropy as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG_EDGE: usize = 320;
    const SHORT_EDGE: usize = 240;
    const NUM_PIXELS: usize = SHORT_EDGE * LONG_EDGE;

    const NOT_USABLE: &[u8] = include_bytes!("../test_data/unusable-320x240.png");
    const USABLE: &[u8] = include_bytes!("../test_data/not-person-320x240.png");

    fn png_pixels(png_bytes: &[u8]) -> image::ImageResult<Vec<u8>> {
        use image::ImageDecoder;
        let mut pixels = vec![0; NUM_PIXELS];
        image::codecs::png::PngDecoder::new(png_bytes)?.read_image(&mut pixels)?;
        Ok(pixels)
    }

    #[test]
    fn test_calculate_entropy_nomiri() {
        let mut image_data = png_pixels(NOT_USABLE).unwrap();
        let e1 = calculate_entropy(&mut image_data);
        assert!(e1 > ENTROPY_THRESHOLD);

        image_data = png_pixels(USABLE).unwrap();
        let e2 = calculate_entropy(&mut image_data);
        assert!(e2 < ENTROPY_THRESHOLD);
    }
}
