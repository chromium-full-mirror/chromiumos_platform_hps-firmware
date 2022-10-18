// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Transposition starting points for 320x240 (or 240x320) matrix.
const STARTING_POINTS: &[usize] = &[
    1, 2, 3, 6, 7, 13, 14, 17, 61, 122, 1259, 2518, 3777, 7554, 76799,
];

/// Returns `x * 320`. This is optimization is only needed because we currently
/// don't have hardware multiplication and division support. If we build our own
/// version of the rust compiler that supports riscv32im, this (and div240)
/// would be unnecessary.
fn mul320(x: usize) -> usize {
    // 320 in binary is 101000000. i.e. bits 8 and 6 are set.
    (x << 8) + (x << 6)
}

macro_rules! div240_bit {
    ($r:ident, $q:ident, $b:expr) => {
        if $r >= 240 << $b {
            $r -= 240 << $b;
            $q += 1 << $b;
        }
    };
}

/// Returns a tuple containing x / 240 and x % 240. Computes the result without
/// using integer division or multiplication. Only supports dividing values up
/// to 240 * 512.
fn div240(x: usize) -> (usize, usize) {
    let mut remainder = x;
    let mut quotient = 0;
    // We start from 8 because  1 << 8 is 512, which is the smallest number >=
    // LONG_EDGE. We could do this with a loop, but manually unrolling this
    // reduces execution time considerably, at least with the compilation
    // settings we're currently using.
    div240_bit!(remainder, quotient, 8);
    div240_bit!(remainder, quotient, 7);
    div240_bit!(remainder, quotient, 6);
    div240_bit!(remainder, quotient, 5);
    div240_bit!(remainder, quotient, 4);
    div240_bit!(remainder, quotient, 3);
    div240_bit!(remainder, quotient, 2);
    div240_bit!(remainder, quotient, 1);
    div240_bit!(remainder, quotient, 0);
    (quotient, remainder)
}

// Rotate anticlockwise with precomputed starting points for 320x240
// (or 240x320) matrix, using follow the cycle algorithm.
fn transpose_320_240(arr: &mut [i8]) {
    for start in STARTING_POINTS.iter().copied() {
        let mut index = start;
        let mut hold_value = arr[start];
        loop {
            let (m, d) = div240(index);
            index = mul320(d) + m;
            core::mem::swap(&mut arr[index], &mut hold_value);
            if index == start {
                break;
            }
        }
    }
}

/// Rotates a 240x320 image to produce a 320x240 image. The resulting image will
/// be mirror-imaged, so it's recommended that input image be flipped vertically
/// in hardware prior to passing to this function so that the output ends up the
/// right way around.
pub fn rotate_240_320_clockwise(arr: &mut [i8]) {
    transpose_320_240(arr);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::png_pixels;

    const SHORT_EDGE: usize = crate::camera::IMAGE_HEIGHT as usize;
    const LONG_EDGE: usize = crate::camera::IMAGE_WIDTH as usize;

    const NOT_PERSON_ROTATED: &[u8] = include_bytes!("../test_data/not-person-240x320.png");
    const NOT_PERSON: &[u8] = include_bytes!("../test_data/not-person-320x240.png");

    // Reverses the elements in each matrix row, regardless of MxN size.
    fn reverse_rows(arr: &mut [i8], cols: usize, rows: usize) {
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

    #[test]
    fn test_mul320() {
        for i in [0, 1, 2, 20, 230, 239] {
            assert_eq!(mul320(i), i * 320);
        }
    }

    #[test]
    fn test_div240() {
        const MAX: usize = LONG_EDGE * SHORT_EDGE;
        for i in [0, 1, 100, 239, 240, 241, MAX - 241, MAX - 240, MAX - 1] {
            let (q, r) = div240(i);
            println!("{}: q: {} vs {} -- r: {} vs {}", i, q, i / 240, r, i % 240);
            assert_eq!(q, i / 240);
            assert_eq!(r, i % 240);
        }
    }
}
