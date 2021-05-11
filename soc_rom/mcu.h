// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <stdint.h>

// Platform specific header file
#ifndef _MCU_SPI_H_
#define _MCU_SPI_H_

// Global send and receive buffers
#define MCU_BUFFER_LEN 64
extern uint8_t mcu_buf_out[MCU_BUFFER_LEN];
extern uint8_t mcu_buf_in[MCU_BUFFER_LEN];

// Transacts with the MCU
//
// Sends the content of spi_mcu_buf_out and waits for a response to be received
// into spi_mcu_buf_in.
//
// Returns 0 on success and -1 on failure
int mcu_transaction();

// Number of times the controller will poll the MCU for a response before
// giving up.
#define MCU_POLL_COUNT 1000

#endif  // _MCU_SPI_H_
