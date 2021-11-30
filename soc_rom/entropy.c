// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "entropy.h"

#include "camera.h"

// Calculates an approximation of ln(x)/ln(2) * 2^24.
//
// Accurately calculates the integer portion of ln(x)/ln(2), then relies on
// the approximation ln(x) ~= (x-1) to calculate the fractional part of the base
// 2 logarithm. Bit shifting is used to pack integer and fraction into a 32 bit
// result that is a fixed point number, with the integer part in the top 8 bits
// and fractional part in the lower 24 bits.
// The resulting output is accurate to within 6%, with error (generally)
// decreasing for larger x."
static uint32_t fixed_log2(uint32_t x) {
  uint32_t shift = 24;
  uint32_t exp = 0;
  while (x >= (uint32_t)(1 << (exp + 1))) {
    exp += 1;
  }
  uint32_t frac = x - (1 << exp);
  return (exp << shift) + (frac << (shift - exp));
}

// This function calculates if the most recently taken image is usable.
// Images which contain most of the pixels in a small number of buckets
// will have much greater entropy value.
uint32_t calculate_entropy(int8_t* buffer) {
  uint32_t histogram[256] = {0};
  for (int i = 0; i < CAM_IMAGE_WIDTH * CAM_IMAGE_HEIGHT; i++) {
    histogram[buffer[i]]++;
  }
  // This method of calculating entropy is very custom and it has been
  // achieved and tested using Rust adequate.
  // For more details, please refer to b/204518775#comment9.
  uint64_t entropy = 0;
  for (int i = 0; i < 256; i++) {
    if (histogram[i] == 0) {
      continue;
    }
    uint64_t value = histogram[i];
    // To preserve some of the fraction value, smaller shift is applied.
    entropy += ((value * fixed_log2(value)) >> 16);
  }
  return entropy;
}
