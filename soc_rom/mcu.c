// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "mcu.h"

#include <generated/csr.h>
#include <stdio.h>
#include <string.h>

// Global buffer definitions
uint8_t mcu_buf_out[MCU_BUFFER_LEN];
uint8_t mcu_buf_in[MCU_BUFFER_LEN];

static void dump_buffer(uint8_t* buf) {
  for (int i = 0; i < MCU_BUFFER_LEN; i++) {
    printf("%02x%c", buf[i], (i & 0xf) == 15 ? '\n' : ' ');
  }
}

// Wait until transmit buffer empty, then write a byte
static void write_1(uint8_t b) {
  while (!mcu_spi_flags_txe_read())
    /* wait */;
  mcu_spi_txd_write(b);
}

// Wait until receive buffer not empty, then read a byte
static uint8_t read_1() {
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

// Do a transaction with the MCU
// TODO: lots of error handling
int mcu_transaction() {
  puts("MCU Transaction sending:");
  dump_buffer(mcu_buf_out);

  // Transfer to MCU
  spi_xfer(mcu_buf_out, mcu_buf_in);

  // Receive from MCU until first byte not 0
  for (int count = 0; count < MCU_POLL_COUNT; count++) {
    if (mcu_buf_in[0] != 0) {
      puts("Received OK:");
      dump_buffer(mcu_buf_in);
      return 0;
    } else {
      printf("MCU Poll: %d\n", count + 1);
      memset(mcu_buf_in, 0, MCU_BUFFER_LEN);
      spi_xfer(mcu_buf_in, mcu_buf_in);
    }
  }
  puts("MCU Transaction failed");
  return -1;
}
