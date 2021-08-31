// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Platform specific header file
#ifndef _TRANSPOSE_H_
#define _TRANSPOSE_H_

#include <stddef.h>
#include <stdint.h>

// This function transposes a matrix of 320x240 (or 240x320) and reverses the
// rows values. These two operations are equal to rotating an image by 90
// degrees clockwise. Function is required for HPS camera modules, which are
// installed sideways.
void rotate_320_240_clockwise(uint8_t*);
void rotate_240_320_clockwise(uint8_t*);

#endif  // _TRANSPOSE_H_