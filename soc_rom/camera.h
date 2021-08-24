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

typedef struct {
    uint16_t    ui16Reg;
    uint16_t    ui8Val;
} init_script;

// TODO(b/196113557): Camera configuration should be moved to MCU.
// Initial HM01B0 configuration provided by vendor with vertical flip.
static const init_script hps_init[] = {
    {0x0103, 0x00,}, // Software reset.
    {0x0100, 0x00,}, // Power up.
    // Analog configuration.
    {0x1003, 0x08},
    {0x1007, 0x08},
    {0x3044, 0x0A},
    {0x3045, 0x00},
    {0x3047, 0x0A},
    {0x3050, 0xC0},
    {0x3051, 0x42},
    {0x3052, 0x50},
    {0x3053, 0x00},
    {0x3054, 0x03},
    {0x3055, 0xF7},
    {0x3056, 0xF8},
    {0x3057, 0x29},
    {0x3058, 0x1F},
    {0x3059, 0x1E},
    {0x3064, 0x00},
    {0x3065, 0x04},
    // Black level configuration (BLC).
    {0x1000, 0x43},
    {0x1001, 0x40},
    {0x1002, 0x32},
    // Digital gain.
    {0x0350, 0x7F},
    // Black level indicator (BLI).
    {0x1006, 0x01},
    {0x1008, 0x00},
    {0x1009, 0xA0},
    {0x100A, 0x60},
    {0x100B, 0x90},
    {0x100C, 0x40},
    // Sync shift.
    {0x1012, 0x00},
    // Auto-exposure.
    {0x2100, 0x01},
    {0x2104, 0x07},
    {0x2105, 0x02},
    {0x2106, 0x14},
    {0x2108, 0x03},
    {0x2109, 0x03},
    {0x210B, 0x80},
    {0x210F, 0x00},
    {0x2110, 0x85},
    {0x2111, 0x00},
    {0x2112, 0xA0},
    // Frame rate (42 FPS).
    {0x0340, 0x01,}, // Frame length 376 (0x0178).
    {0x0341, 0x78,},
    {0x0342, 0x01,}, // Line length 376 (0x0178).
    {0x0343, 0x78,},
    // Resolution QVGA 324x244.
    {0x3010, 0x01,}, // Enable QVGA.
    {0x0383, 0x01,},
    {0x0387, 0x01,},
    {0x0390, 0x00,},
    {0x0101, 0x01,}, // Enable vertical flip.
    // Bit width selection.
    {0x3011, 0x70,}, // 8-bit mode.
    {0x3059, 0x02,},
    {0x3060, 0x20,}, // Enable gated CLK.
    // CMU update.
    {0x0104, 0x00,},
    // Standby mode.
    {0x0100, 0x00,},
};

#endif