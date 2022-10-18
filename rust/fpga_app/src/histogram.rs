// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::camera::IMAGE_HEIGHT;
use crate::camera::IMAGE_WIDTH;
use core::fmt;

/// A Histogram of counts of pixel values
#[derive(Debug, PartialEq, Eq)]
pub struct Histogram {
    // Binned data
    // bin[0] stores counts for pixels of value -128
    // bin[1] stores counts for -127, and so on up to
    // bin[255] stores counts for pixels of value +127
    bins: [u32; 256],
}

/// A bin from a histogram - used when iterating over histogram values
#[derive(Clone, Copy)]
struct Bin {
    value: i8,
    count: u32,
}

impl Default for Histogram {
    fn default() -> Self {
        Self { bins: [0u32; 256] }
    }
}

impl Histogram {
    pub fn new(image_data: &[i8]) -> Histogram {
        let mut result = Histogram::default();

        for &val in image_data {
            result.add(val);
        }
        result
    }

    /// Histogram from central 240x240 square in image. Within the selected
    /// square only every 4th pixel is sampled. We sample every 4th pixel since
    /// that is sufficient for our purposes and by not sampling every pixel, the
    /// camera code can produce a histogram during image capture. If the camera
    /// code tried to sample every pixel, it would be too slow and would drop
    /// pixels, resulting in a corrupted image.
    pub fn new_cropped(image_data: &[i8]) -> Histogram {
        let mut result = Histogram { bins: [0; 256] };

        for y in 0..IMAGE_HEIGHT {
            for x in (40..IMAGE_WIDTH - 40).step_by(4) {
                let i = (y * IMAGE_WIDTH + x) as usize;
                result.add(image_data[i]);
            }
        }
        result
    }

    /// Adds a single value to the histogram
    #[inline(always)]
    pub fn add(&mut self, value: i8) {
        let index = (value as i16 + 128) as usize;
        self.bins[index] += 1;
    }

    /// Iterator over bin indices and values
    fn bin_iter(&self) -> impl Iterator<Item = Bin> + '_ {
        self.bins.iter().enumerate().map(|(index, count)| Bin {
            value: ((index as i16) - 128) as i8,
            count: *count,
        })
    }

    /// Returns the number of values added to this histogram.
    pub fn count(&self) -> u32 {
        self.bins.iter().sum()
    }

    /// Median pixel value, ignoring extreme pixel values
    pub fn median(&self) -> i8 {
        let mut running_total = 0;
        let target = self.count() / 2;
        for bin in self.bin_iter() {
            running_total += bin.count;
            if running_total > target {
                return bin.value;
            }
        }
        panic!("Median not found");
    }

    /// Simplified, eight bin histogram for debugging display
    /// Return the percentage of values in each bin
    fn display_bins(&self) -> [u32; 8] {
        const BIN_ENDS: [i8; 8] = [-120, -80, -40, 0, 40, 80, 120, 127];

        // First create an 8 bin histogram
        let mut bin8 = [0u32; 8];
        let mut bin8_index = 0;
        for raw_bin in self.bin_iter() {
            bin8[bin8_index] += raw_bin.count;
            if raw_bin.value == BIN_ENDS[bin8_index] {
                bin8_index += 1;
            }
        }

        // Now calculate the percentages in each bin
        let mut result = [0u32; 8];
        let count: u32 = bin8.iter().sum();
        for i in 0..8 {
            result[i] = bin8[i] * 100 / count;
        }
        result
    }

    /// Determines whether the image that was used to create this Histogram
    /// is suitable for use by the ML model.
    pub fn is_usable(&self) -> bool {
        // Images with entropy below the threshold are usable.
        // The entropy threshold was defined empirically.
        // For more details, please refer to b/204518775#comment9.
        const ENTROPY_THRESHOLD: u32 = 31000000;
        self.entropy() < ENTROPY_THRESHOLD
    }

    /// Calculates image entropy
    fn entropy(&self) -> u32 {
        // See b/204518775#comment9 for details on this calculation
        let mut entropy: u32 = 0;
        for bin in self.bin_iter() {
            if bin.count != 0 {
                // To preserve some of the fraction value, smaller shift is applied.
                let c = bin.count;
                entropy += ((c as u64 * fixed_log2(c) as u64) >> 16) as u32;
            }
        }
        entropy
    }
}

// Calculates an approximation of ln(x)/ln(2) * 2^24.
//
// Accurately calculates the integer portion of ln(x)/ln(2), then relies on
// the approximation ln(x) ~= (x-1) to calculate the fractional part of the base
// 2 logarithm. Bit shifting is used to pack integer and fraction into a 32 bit
// result that is a fixed point number, with the integer part in the top 8 bits
// and fractional part in the lower 24 bits.
// The resulting output is accurate to within 6%, with error (generally)
// decreasing for larger x."
fn fixed_log2(x: u32) -> u32 {
    let shift = 24;
    let mut exp = 0;
    while x >= (1 << (exp + 1)) {
        exp += 1;
    }
    let frac: u32 = x - (1 << exp);
    (exp << shift) + (frac << (shift - exp))
}

impl fmt::Display for Histogram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bins = self.display_bins();
        write!(
            f,
            "m:{:4} h:{:3}{:3}{:3}{:3}{:3}{:3}{:3}{:3} e:{} {}",
            self.median(),
            bins[0],
            bins[1],
            bins[2],
            bins[3],
            bins[4],
            bins[5],
            bins[6],
            bins[7],
            self.entropy(),
            self.is_usable(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_DATA: &[i8] = &[-125, -65, -65, -55, 3, 55, 75, 77, 125];

    use crate::camera::NUM_PIXELS;
    use crate::test_helpers::images;
    use crate::test_helpers::png_pixels;

    #[test]
    fn test_calculate_entropy_nomiri() {
        assert!(!Histogram::new_cropped(&png_pixels(images::WHITE).unwrap()).is_usable());
        assert!(Histogram::new_cropped(&png_pixels(images::NOT_PERSON).unwrap()).is_usable());

        // An image with all pixels of the same value is not usable
        assert!(!Histogram::new_cropped(&[25; 76800]).is_usable());
    }

    #[test]
    fn test_fixed_log2() {
        assert_eq!(0, fixed_log2(1));
        assert_eq!(1 << 24, fixed_log2(2));
        assert_eq!(0b11 << 23, fixed_log2(3));
        assert_eq!(2 << 24, fixed_log2(4));

        assert_eq!(8 << 24, fixed_log2(256));
        assert_eq!(0b011111111110 << 16, fixed_log2(255));
        assert_eq!(0b100000000000 << 16, fixed_log2(256));
        assert_eq!(0b100000000001 << 16, fixed_log2(257));
    }

    // Checks the number of values in buckets 0 and 255, and that all
    // other bins contain 0 values.
    fn check_histogram(h: &Histogram, b0: u32, b255: u32) {
        assert_eq!(h.bins[0], b0);
        for i in 1..254 {
            assert_eq!(h.bins[i], 0);
        }
        assert_eq!(h.bins[255], b255);
    }

    #[test]
    fn test_median() {
        let h = Histogram::new(TEST_DATA);
        assert_eq!(3, h.median());
    }

    #[test]
    fn test_histogram_nomiri() {
        // Create i8 vector representing a dark 240x240 square in center of
        // light 320x240 image
        let mut img: [i8; NUM_PIXELS] = [127; NUM_PIXELS];
        for y in 0..IMAGE_HEIGHT {
            for x in 40..IMAGE_WIDTH - 40 {
                let index = (y * IMAGE_WIDTH + x) as usize;
                img[index] = -128;
            }
        }

        // Full histogram has some dark and some light pixels
        let h_full = Histogram::new(&img);
        check_histogram(&h_full, 240 * IMAGE_HEIGHT, 80 * IMAGE_HEIGHT);

        // Cropped histogram only has dark pixels
        let h_cropped = Histogram::new_cropped(&img);
        check_histogram(&h_cropped, 240 * IMAGE_HEIGHT / 4, 0);
    }

    #[test]
    fn test_calculate_median_nomiri() {
        let h = Histogram::new(TEST_DATA);
        assert_eq!(3, h.median());

        // The median of cropped image should not be same as full frame.
        assert_ne!(
            Histogram::new(&png_pixels(images::NOT_PERSON).unwrap()).median(),
            Histogram::new_cropped(&png_pixels(images::NOT_PERSON).unwrap()).median()
        );
        // The median of entire white image should be the same.
        assert_eq!(
            Histogram::new(&png_pixels(images::WHITE).unwrap()).median(),
            Histogram::new_cropped(&png_pixels(images::WHITE).unwrap()).median()
        );
    }

    #[test]
    fn test_display_bins() {
        let h = Histogram::new(TEST_DATA);
        const EXPECTED: [u32; 8] = [11, 0, 33, 0, 11, 33, 0, 11];
        assert_eq!(EXPECTED, h.display_bins());
    }
}
