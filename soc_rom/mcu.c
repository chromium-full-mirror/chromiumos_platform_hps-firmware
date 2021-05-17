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

// Do a transaction with the MCU
// TODO: lots of error handling
int mcu_transaction(void) {
  // Transfer to MCU
  spi_xfer(mcu_buf_out, mcu_buf_in);

  // Receive from MCU until first byte not 0
  for (int count = 0; count < MCU_POLL_COUNT; count++) {
    if (mcu_buf_in[0] != 0) {
      return 0;
    } else {
      // A short delay before we try another SPI transfer, otherwise the CS line
      // won't be high for long enough. This value was found experimentally to
      // be sufficient.
      delay_us(1);
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
  vsnprintf((char*)&mcu_buf_out[1], MCU_BUFFER_LEN - 1, format, args);
  mcu_buf_out[0] = MCU_COMMAND_SET_DEBUG_LOG;
  va_end(args);
  mcu_transaction();
}
