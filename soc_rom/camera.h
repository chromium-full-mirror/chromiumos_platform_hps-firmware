// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Platform specific header file
#ifndef _CAMERA_I2C_H_
#define _CAMERA_I2C_H_

#include <stddef.h>
#include <stdint.h>

// Read camera registers using I2C bus.
// The address is 2B size. The value is 1B size. Returns 0 if successful.
int read_camera_i2c(uint16_t*, int, uint8_t*);

// Write to camera registers using I2C bus.
// The address is 2B size. The value is 1B size. Returns 0 if successful.
int write_camera_i2c(uint16_t*, int, uint8_t*);

// Print a value stored at a single camera register.
// The address is 2B size. The value is 1B size.
void print_camera_register(uint16_t);

#endif