// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// For more details about the algorithm, please refer to
// https://en.wikipedia.org/wiki/In-place_matrix_transposition

#include "transpose.h"

#define LONG_EDGE 320
#define SHORT_EDGE 240

// Number of transposition starting points in 320x240 (or 240x320) matrix.
#define STARTING_POINTS_LEN (size_t)16
// Transposition starting points for 320x240 (or 240x320) matrix.
static uint32_t STARTING_POINTS[STARTING_POINTS_LEN] = {
    1, 2, 3, 6, 7, 13, 14, 17, 61, 122, 1259, 2518, 3777, 7554, 76799,
};

// Rotate anticlockwise with precomputed starting points for 320x240
// (or 240x320) matrix, using follow the cycle algorithm.
static void transpose_320_240(uint8_t* arr, uint32_t arr_x, uint32_t arr_y) {
  uint8_t hold_value;
  for (uint32_t s = 0; s < STARTING_POINTS_LEN; s++) {
    uint32_t start = STARTING_POINTS[s];
    hold_value = arr[start];
    do {
      start = (start % arr_x) * arr_y + start / arr_x;
      uint8_t tmp = arr[start];
      arr[start] = hold_value;
      hold_value = tmp;
    } while (STARTING_POINTS[s] != start);
  }
}

// Reverses the elements in each matrix row, regardless of MxN size.
static void reverse_rows(uint8_t* arr, uint32_t cols, uint32_t rows) {
  uint8_t tmp;
  for (uint32_t r = 0; r < rows; r++) {
    int end = cols - 1;
    for (uint32_t start = 0; start < cols / 2; start++) {
      tmp = arr[r * cols + start];
      arr[r * cols + start] = arr[r * cols + end];
      arr[r * cols + end] = tmp;
      end--;
    }
  }
}

void rotate_320_240_clockwise(uint8_t* arr) {
  transpose_320_240(arr, LONG_EDGE, SHORT_EDGE);
  // Transposed matrix has new dimensions (MxN is now NxM).
  reverse_rows(arr, SHORT_EDGE, LONG_EDGE);
}

void rotate_240_320_clockwise(uint8_t* arr) {
  transpose_320_240(arr, SHORT_EDGE, LONG_EDGE);
  // Transposed matrix has new dimensions (MxN is now NxM).
  reverse_rows(arr, LONG_EDGE, SHORT_EDGE);
}