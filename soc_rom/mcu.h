// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <stddef.h>
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
  MCU_COMMAND_SET_DEBUG_LOG = 0x61,
  MCU_COMMAND_SEND_IMAGE = 0x62,
  MCU_COMMAND_GET_DEBUG_CMD = 0x63,
};

// The first byte of each received packet is a status code.
enum mcu_response_status {
  MCU_RESPONSE_NOT_READY = 0,
  MCU_RESPONSE_READY = 1,  // Sent in response to a poll.
  MCU_RESPONSE_OK = 2,     // Sent in response to any other command.
};

// Debug commands. These aren't intended for production use, only for debugging.
// If we want equivalent things for production use, then they should just be
// bits in the configuration struct.
enum debug_command {
  DEBUG_CMD_NONE = 0,
  DEBUG_CMD_CLASSIFY_OFF = 1,
  DEBUG_CMD_CLASSIFY_ON = 2,
  DEBUG_CMD_TRANSFER_IMAGE_OFF = 3,
  DEBUG_CMD_TRANSFER_IMAGE_ON = 4,
  DEBUG_CMD_RUN_EXPOSURE_CALIBRATION = 5,
  DEBUG_CMD_SET_ANALOG_GAIN = 6,
  DEBUG_CMD_SET_DIGITAL_GAIN = 7,
  DEBUG_CMD_SET_INTEGRATION = 8,
  DEBUG_CMD_SET_CUSTOM_CAM_REG_ADDRESS = 9,
  DEBUG_CMD_SET_CUSTOM_CAM_REG_VALUE = 10,
  DEBUG_CMD_READ_CAM_REG = 11,
  DEBUG_CMD_PRINT_CAM_GAINS = 12,
  DEBUG_CMD_SET_AUTOMATIC_EXPOSURE_TARGET = 13,
};

// Global send and receive buffers
#define MCU_BUFFER_LEN 64
extern uint8_t mcu_buf_out[MCU_BUFFER_LEN];
extern uint8_t mcu_buf_in[MCU_BUFFER_LEN];

// Initializes SPI. Must be called on startup before mcu_transaction or
// mcu_printf are called.
void spi_init(void);

// Transacts with the MCU
//
// Sends the content of spi_mcu_buf_out and waits for a response to be received
// into spi_mcu_buf_in.
//
// Returns 0 on success and -1 on failure
int mcu_transaction(void);

// Like printf, but the output will show up in the MCU's debug output, if there
// is a debug probe connected to the MCU.
void mcu_printf(const char* format, ...) __attribute__((format(printf, 1, 2)));

// Number of times the controller will poll the MCU for a response before
// giving up.
#define MCU_POLL_COUNT 1000

struct mcu_stream {
  uint8_t* out;
  size_t remaining_capacity;
  enum mcu_command command;
};

// Initializes a stream for the supplied command. While the stream is active, no
// other MCU SPI transactions should be performed (since they all share a common
// buffer). This includes mcu_printf.
void mcu_stream_initialize(struct mcu_stream* stream, enum mcu_command command);

// Sends `data` to `stream`. Returns 0 on success or -1 on failure. Data may not
// be sent immediately. mcu_stream_flush must be called once done to ensure that
// all data is sent..
int mcu_stream_send_data(struct mcu_stream* stream,
                         const uint8_t* data,
                         size_t len);

// Flushes `stream`. The last packet of data will be padded out with zeros if
// there is unoccupied space. Returns 0 on success or -1 on failure.
int mcu_stream_flush(struct mcu_stream* stream);

#endif  // _MCU_SPI_H_
