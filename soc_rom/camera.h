// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Platform specific header file
#ifndef _CAMERA_I2C_H_
#define _CAMERA_I2C_H_

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#define HM01B0_MODE_SELECT 0x0100
#define HM01B0_FRAME_COUNT 0x3020
#define HM01B0_AE_TARGET_MEAN 0x2101
#define HM01B0_AE_MEAN 0x2020

#define CAM_IMAGE_WIDTH 320
#define CAM_IMAGE_HEIGHT 240

// Read camera registers using I2C bus.
// The address is 2B size. The value is 1B size. Returns 0 if successful.
int read_camera_i2c(uint16_t*, int, uint8_t*);

// Write to camera registers using I2C bus.
// The address is 2B size. The value is 1B size. Returns 0 if successful.
int write_camera_i2c(uint16_t*, int, const uint8_t*);

// Wait for the camera to return to standby mode.
void wait_for_camera_standby(void);

// Set streaming mode for the camera for given number of frames.
void set_camera_streaming_mode(uint8_t);

// Capture an image
void capture(bool rotate_image, int8_t* destination);

#endif