// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <stdint.h>

// Platform specific header file
#ifndef _MCU_SPI_H_
#define _MCU_SPI_H_

// The first byte of each transmitted packet is a command.
enum mcu_command {
  MCU_COMMAND_POLL = 0,
  MCU_COMMAND_READ_CONFIG_REGISTERS = 0x20,
  MCU_COMMAND_WRITE_STATUS_REGISTERS = 0x21,
  MCU_COMMAND_RESET_FPGA = 0x40,
  MCU_COMMAND_SET_DEBUG_LED = 0x60,
};

// The first byte of each received packet is a status code.
enum mcu_response_status {
  MCU_RESPONSE_NOT_READY = 0,
  MCU_RESPONSE_READY = 1,  // Sent in response to a poll.
  MCU_RESPONSE_OK = 2,     // Sent in response to any other command.
};

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
int mcu_transaction(void);

// Number of times the controller will poll the MCU for a response before
// giving up.
#define MCU_POLL_COUNT 1000

#endif  // _MCU_SPI_H_
