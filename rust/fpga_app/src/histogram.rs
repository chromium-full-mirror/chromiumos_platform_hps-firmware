// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::fmt;

/// A Histogram of counts of pixel values
pub struct Histogram {
    // Total of bins = number of pixels used to construct histogram
    count: u32,
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

impl Histogram {
    pub fn new(image_data: &[i8]) -> Histogram {
        let mut result = Histogram {
            count: 0,
            bins: [0; 256],
        };

        for &val in image_data {
            result.add(val);
        }

        result
    }

    /// Adds a single value to the histogram
    fn add(&mut self, value: i8) {
        let index = (value as i16 + 128) as usize;
        self.bins[index] += 1;
        self.count += 1;
    }

    /// Iterator over bin indices and values
    fn bin_iter(&self) -> impl Iterator<Item = Bin> + '_ {
        self.bins.iter().enumerate().map(|(index, count)| Bin {
            value: ((index as i16) - 128) as i8,
            count: *count,
        })
    }

    /// Median pixel value, ignoring extreme pixel values
    fn median(&self) -> i8 {
        let mut running_total = 0;
        let target = self.count / 2;
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
        for i in 0..8 {
            result[i] = bin8[i] * 100 / self.count;
        }
        result
    }
}

impl fmt::Display for Histogram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bins = self.display_bins();
        write!(
            f,
            "m:{:4} h:{:3}{:3}{:3}{:3}{:3}{:3}{:3}{:3}",
            self.median(),
            bins[0],
            bins[1],
            bins[2],
            bins[3],
            bins[4],
            bins[5],
            bins[6],
            bins[7],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DATA: &[i8] = &[-125, -65, -65, -55, 3, 55, 75, 77, 125];

    #[test]
    fn test_calculate_median() {
        let h = Histogram::new(TEST_DATA);
        assert_eq!(3, h.median());
    }

    #[test]
    fn test_display_bins() {
        let h = Histogram::new(TEST_DATA);
        const EXPECTED: [u32; 8] = [11, 0, 33, 0, 11, 33, 0, 11];
        assert_eq!(EXPECTED, h.display_bins());
    }
}
