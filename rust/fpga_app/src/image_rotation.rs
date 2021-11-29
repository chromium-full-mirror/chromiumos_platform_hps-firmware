// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

const LONG_EDGE: usize = 320;
const SHORT_EDGE: usize = 240;

// Transposition starting points for 320x240 (or 240x320) matrix.
const STARTING_POINTS: &[usize] = &[
    1, 2, 3, 6, 7, 13, 14, 17, 61, 122, 1259, 2518, 3777, 7554, 76799,
];

// Rotate anticlockwise with precomputed starting points for 320x240
// (or 240x320) matrix, using follow the cycle algorithm.
fn transpose_320_240(arr: &mut [u8], arr_x: usize, arr_y: usize) {
    for start in STARTING_POINTS.iter().copied() {
        let mut index = start;
        let mut hold_value = arr[start];
        loop {
            index = (index % arr_x) * arr_y + index / arr_x;
            core::mem::swap(&mut arr[index], &mut hold_value);
            if index == start {
                break;
            }
        }
    }
}

/// Rotates a 240x320 image to produce a 320x230 image. The resulting image will
/// be mirror-imaged, so it's recommended that input image be flipped vertically
/// in hardware prior to passing to this function so that the output ends up the
/// right way around.
pub fn rotate_240_320_clockwise(arr: &mut [u8]) {
    transpose_320_240(arr, SHORT_EDGE, LONG_EDGE);
}

#[cfg(test)]
mod tests {
    use super::*;

    const NUM_PIXELS: usize = SHORT_EDGE * LONG_EDGE;

    const NOT_PERSON_ROTATED: &[u8] = include_bytes!("../test_data/not-person-240x320.png");
    const NOT_PERSON: &[u8] = include_bytes!("../test_data/not-person-320x240.png");

    fn png_pixels(png_bytes: &[u8]) -> image::ImageResult<Vec<u8>> {
        use image::ImageDecoder;
        let mut pixels = vec![0; NUM_PIXELS];
        image::codecs::png::PngDecoder::new(png_bytes)?.read_image(&mut pixels)?;
        Ok(pixels)
    }

    // Reverses the elements in each matrix row, regardless of MxN size.
    fn reverse_rows(arr: &mut [u8], cols: usize, rows: usize) {
        let mut tmp;
        for r in 0..rows {
            let mut end = cols - 1;
            for start in 0..cols / 2 {
                tmp = arr[r * cols + start];
                arr[r * cols + start] = arr[r * cols + end];
                arr[r * cols + end] = tmp;
                end -= 1;
            }
        }
    }

    // This test is disabled from running with MIRI due to it being too slow.
    // The code under test doesn't use any unsafe though, so this isn't a big
    // loss.
    #[test]
    fn test_rotate_nomiri() {
        let mut buffer = png_pixels(NOT_PERSON_ROTATED).unwrap();
        rotate_240_320_clockwise(&mut buffer);
        reverse_rows(&mut buffer, LONG_EDGE, SHORT_EDGE);
        assert_eq!(buffer, png_pixels(NOT_PERSON).unwrap());
    }
}
