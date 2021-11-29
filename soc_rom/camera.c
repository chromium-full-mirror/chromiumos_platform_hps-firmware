// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "camera.h"
#include "mcu.h"
#include "transpose.h"

#include <generated/csr.h>
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

void wait_for_camera_standby(void) {
  uint8_t value;
  uint16_t address = HM01B0_MODE_SELECT;
  do {
    read_camera_i2c(&address, 1, &value);
  } while (value != 0);
}

void set_camera_streaming_mode(uint8_t nframes) {
  uint16_t addresses[] = {HM01B0_FRAME_COUNT, HM01B0_MODE_SELECT};
  uint8_t values[] = {nframes, 0x03};
  write_camera_i2c(addresses, 2, values);
}

void print_camera_register(uint16_t address) {
  uint8_t value = 0;
  read_camera_i2c(&address, 1, &value);
  mcu_printf("Camera register 0x%02x has value 0x%02x\n", address, value);
}

// Wait for DMA to finish
static void cam_dma_wait(void) {
  while (cam_control_running_read()) {
    /* wait */;
  }
}

// Start camera DMA
static void cam_dma_enable(int8_t* destination,
                           uint16_t column_start,
                           uint16_t column_count,
                           uint16_t row_start,
                           uint16_t row_count) {
  // Force a reset, then wait to get to an idle state
  cam_control_reset_write(1);
  while (!cam_control_idle_read()) {
    /* wait */;
  }

  // Configure with 320x240 window
  cam_control_column_start_write(column_start);
  cam_control_column_count_write(column_count);
  cam_control_row_start_write(row_start);
  cam_control_row_count_write(row_count);
  cam_control_signed_dma_write(1);
  cam_control_base_address_write((uint32_t)destination);

  cam_control_start_run_write(1);
}

void capture(bool rotate_image, int8_t* destination) {
  // Command camera to take a frame
  wait_for_camera_standby();
  if (rotate_image) {
    // Clips 240x320 image from X = [42, 282], Y = [2, 322].
    cam_dma_enable(destination, (324 - CAM_IMAGE_HEIGHT) / 2, CAM_IMAGE_HEIGHT,
                   2, CAM_IMAGE_WIDTH);
  } else {
    // Clips 320x240 image from X = [2, 322], Y = [42, 282].
    cam_dma_enable(destination, 2, CAM_IMAGE_WIDTH,
                   (324 - CAM_IMAGE_HEIGHT) / 2, CAM_IMAGE_HEIGHT);
  }
  set_camera_streaming_mode(1);
  cam_dma_wait();
  if (rotate_image) {
    // Rotates 240x320 image to an expected 320x240 format.
    rotate_240_320_clockwise((uint8_t*)destination);
  }
}
