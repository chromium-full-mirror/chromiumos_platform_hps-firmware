// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "camera.h"
#include "mcu.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

int read_camera_i2c(uint16_t* addresses, int num_addresses, uint8_t* values) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_READ_CAMERA_I2C;
  mcu_buf_out[1] = (uint8_t)num_addresses;
  for (int i = 0; i < num_addresses; i++) {
    mcu_buf_out[i * 2 + 2] = (uint8_t)(addresses[i] >> 8);
    mcu_buf_out[i * 2 + 3] = (uint8_t)(addresses[i] & 0xff);
  }
  mcu_transaction();
  if (mcu_buf_in[0] != MCU_RESPONSE_OK ||
      mcu_buf_in[1] != MCU_COMMAND_READ_CAMERA_I2C) {
    puts("MCU did not acknowledge command");
    return 1;
  }
  for (int i = 0; i < num_addresses; i++) {
    values[i] = mcu_buf_in[i * 2 + 2];
  }
  return 0;
}

int write_camera_i2c(uint16_t* addresses, int num_addresses, uint8_t* values) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_WRITE_CAMERA_I2C;
  mcu_buf_out[1] = (uint8_t)num_addresses;
  for (int i = 0; i < num_addresses; i++) {
    mcu_buf_out[i * 3 + 2] = (uint8_t)(addresses[i] >> 8);
    mcu_buf_out[i * 3 + 3] = (uint8_t)(addresses[i] & 0xff);
    mcu_buf_out[i * 3 + 4] = values[i];
  }
  mcu_transaction();
  if (mcu_buf_in[0] != MCU_RESPONSE_OK ||
      mcu_buf_in[1] != MCU_COMMAND_WRITE_CAMERA_I2C) {
    puts("MCU did not acknowledge command");
    return 1;
  }
  return 0;
}

void print_camera_register(uint16_t address) {
  uint8_t value = 0;
  read_camera_i2c(&address, 1, &value);
  mcu_printf("Camera register 0x%02x has value 0x%02x\n", address, value);
}