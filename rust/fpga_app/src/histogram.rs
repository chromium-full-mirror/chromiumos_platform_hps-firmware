// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::fmt;

/// There are eight bins in the histogram, so we use 7 indices
pub const BIN_ENDS: [i8; 7] = [-120, -80, -40, 0, 40, 80, 120];

/// A Histogram of eight bins over image data.
pub struct Histogram {
    // Total of bins
    count: u32,
    // Binned data
    bins: [u32; 8],
}

impl Histogram {
    pub fn new(image_data: &[u8]) -> Histogram {
        let mut result = Histogram {
            count: 0,
            bins: [0; 8],
        };

        for val in image_data {
            result.add(*val as i8);
        }

        result
    }

    /// Adds a single value to the histogram
    fn add(&mut self, value: i8) {
        let index = BIN_ENDS.iter().position(|end| value <= *end).unwrap_or(7);
        self.bins[index] += 1;
        self.count += 1;
    }

    /// Gets the percentage of values in each of the eight bins
    pub fn bins_percent(&self) -> [u32; 8] {
        let mut result = [0; 8];
        for i in 0..8 {
            result[i] = self.bins[i] * 100 / self.count;
        }
        result
    }
}

impl fmt::Display for Histogram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bins = self.bins_percent();
        write!(
            f,
            "{:3}{:3}{:3}{:3}{:3}{:3}{:3}{:3}",
            bins[0], bins[1], bins[2], bins[3], bins[4], bins[5], bins[6], bins[7],
        )
    }
}
