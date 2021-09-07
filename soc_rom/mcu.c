// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "mcu.h"

#include <generated/csr.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

// Global buffer definitions
uint8_t mcu_buf_out[MCU_BUFFER_LEN];
uint8_t mcu_buf_in[MCU_BUFFER_LEN];
static int spi_enabled = 0;

// Wait until transmit buffer empty, then write a byte
static void write_1(uint8_t b) {
  while (!mcu_spi_flags_txe_read())
    /* wait */;
  mcu_spi_txd_write(b);
}

// Wait until receive buffer not empty, then read a byte
static uint8_t read_1(void) {
  while (mcu_spi_flags_rxe_read())
    /* wait */;
  return mcu_spi_rxd_read();
}

static void spi_xfer(const uint8_t* tx_buffer, uint8_t* rx_buffer) {
  // Wait until MCU is ready
  while (mcu_spi_flags_ready_read() == 0) {
  }

  // Pull CS low
  mcu_spi_cr_cs_write(0);

  // Turn on SPI
  mcu_spi_cr_en_write(1);

  // Send and receive
  for (int i = 0; i < MCU_BUFFER_LEN; i++) {
    write_1(tx_buffer[i]);
    rx_buffer[i] = read_1();
  }

  // Turn off SPI
  mcu_spi_cr_en_write(0);

  // Set CS high
  mcu_spi_cr_cs_write(1);
}

static void delay_us(int us) {
  // Scale was determined experimentally by first setting to 1000, then
  // measuring the delay caused by calling delay_us(100) with an oscilloscope.
  const int SCALE = 2950;
  int iterations = CONFIG_CLOCK_FREQUENCY * us / 1000 / SCALE;
  for (int i = 0; i < iterations; i++) {
    // Prevent loop from being optimized away.
    __asm__("");
  }
}

void spi_init(void) {
  // If the MCU is not ready when we start, then the MCU is probably either not
  // flashed, or not flashed with recent firmware. Either way, we disable SPI
  // transfers.
  // TODO(b/189781227): Remove this from production code.
  for (int i = 0; i < 250; i++) {
    if (mcu_spi_flags_ready_read()) {
      spi_enabled = 1;
      return;
    }
    // Retry afer a 1ms delay up to 250 times.
    delay_us(1000);
  }
  if (!spi_enabled) {
    puts("MCU not ready for SPI transfer. SPI disabled");
  }
}

// Do a transaction with the MCU
// TODO: lots of error handling
int mcu_transaction(void) {
  if (!spi_enabled) {
    return -1;
  }
  // Transfer to MCU
  spi_xfer(mcu_buf_out, mcu_buf_in);

  // Receive from MCU until first byte not 0
  for (int count = 0; count < MCU_POLL_COUNT; count++) {
    if (mcu_buf_in[0] != 0) {
      return 0;
    } else {
      memset(mcu_buf_in, 0, MCU_BUFFER_LEN);
      spi_xfer(mcu_buf_in, mcu_buf_in);
    }
  }
  puts("MCU Transaction failed");
  return -1;
}

void mcu_printf(const char* format, ...) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  va_list args;

  va_start(args, format);
  vprintf(format, args);
  mcu_buf_out[0] = MCU_COMMAND_SET_DEBUG_LOG;
  va_end(args);

  va_start(args, format);
  vsnprintf((char*)&mcu_buf_out[1], MCU_BUFFER_LEN - 1, format, args);
  mcu_buf_out[0] = MCU_COMMAND_SET_DEBUG_LOG;
  va_end(args);
  mcu_transaction();
}

void mcu_vprintf(const char* format, va_list args) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  vsnprintf((char*)&mcu_buf_out[1], MCU_BUFFER_LEN - 1, format, args);
  mcu_buf_out[0] = MCU_COMMAND_SET_DEBUG_LOG;
  mcu_transaction();
}

void mcu_stream_initialize(struct mcu_stream* stream,
                           enum mcu_command command) {
  stream->out = mcu_buf_out + 1;
  stream->remaining_capacity = MCU_BUFFER_LEN - 1;
  stream->command = command;
}

int mcu_stream_send_data(struct mcu_stream* stream,
                         const uint8_t* data,
                         size_t len) {
  while (len > 0) {
    size_t to_send =
        stream->remaining_capacity < len ? stream->remaining_capacity : len;
    memcpy(stream->out, data, to_send);
    stream->out += to_send;
    stream->remaining_capacity -= to_send;
    data += to_send;
    len -= to_send;
    // We only send if there's more data needing to be writen. This allows
    // StreamFlush to assume that it always needs to send.
    if (stream->remaining_capacity == 0 && len > 0) {
      if (mcu_stream_flush(stream) == -1) {
        return -1;
      }
    }
  }
  return 0;
}

int mcu_stream_flush(struct mcu_stream* stream) {
  memset(stream->out, 0, stream->remaining_capacity);
  mcu_buf_out[0] = stream->command;
  if (mcu_transaction() == -1) {
    return -1;
  }
  mcu_stream_initialize(stream, stream->command);
  return 0;
}
