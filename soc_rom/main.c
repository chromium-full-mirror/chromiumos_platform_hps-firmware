// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <console.h>
#include <generated/csr.h>
#include <hps_i2c.h>
#include <hw/common.h>
#include <irq.h>
#include <stdio.h>
#include <string.h>

#include "camera.h"
#include "cat_picture.h"
#include "entropy.h"
#include "generated/csr.h"
#include "hps_version.h"
#include "mcu.h"
#include "litex_shim/shim.h"

static const uint8_t ae_target = 100;
// This version is exposed via an I2C register and has no semantic meaning. It's
// bumped whenever a developer decides to bump it and is just for development
// and debugging purposes. Maximum supported value is currently 255, although
// expanding that to a 16 bit value is trivial.
#define SOC_ROM_VERSION 1

#define CALIBRATION_FRAMES 15

// Images with entropy below the threshold are usable.
// The entropy threshold was defined empirically.
// For more details, please refer to b/204518775#comment9.
#define ENTROPY_THRESHOLD 270000000

struct configuration {
  uint16_t enabled_features;
};

struct status {
  uint16_t person_status;
  uint16_t enabled_features;
  uint16_t loop_count;
  uint16_t second_person_status;
};

// Write the target auto exposure into the camera
void write_ae_cfg(void) {
  static int last_target = -1;
  if (last_target != ae_target) {
    uint16_t address = HM01B0_AE_TARGET_MEAN;
    write_camera_i2c(&address, 1, &ae_target);
    last_target = ae_target;
  }
}

// Stream nframes to allow calibration
static void calibrate(int nframes) {
  wait_for_camera_standby();
  mcu_printf("Calibrating with %d frames\n", nframes);
  set_camera_streaming_mode(nframes);
  wait_for_camera_standby();
}

// Recalibration is required when AE mean is far from the AE target
static int calibration_required(void) {
  static int recalibration_threshold = 0;
  if (recalibration_threshold < 5) {
    recalibration_threshold++;
  }
  uint16_t address = HM01B0_AE_MEAN;
  uint8_t ae_mean = 0;
  read_camera_i2c(&address, 1, &ae_mean);
  // If the autoexposure mean value is 20 steps away from the target there was
  // a major light change in the environment and the recalibration is required.
  // Threshold makes sure that we don't trigger recalibration on every single
  // cycle. Both values are set based on the experiments.
  if (abs(ae_target - ae_mean) > 20 && recalibration_threshold == 5) {
    recalibration_threshold = 0;
    return 1;
  }
  return 0;
}

// Sends status register content to the mcu
static void write_mcu_status(const struct status* status) {
  puts("Updating status in MCU");
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_WRITE_STATUS_REGISTERS;
  memcpy(&mcu_buf_out[1], status, sizeof(*status));
  mcu_transaction();
  if (mcu_buf_in[0] != MCU_RESPONSE_OK ||
      mcu_buf_in[1] != MCU_COMMAND_WRITE_STATUS_REGISTERS) {
    puts("MCU did not acknowledge command");
  }
}

// Performs the classification.
//
// Assumes input data vector is already filled.
int8_t run_classify(int model_number) {
  mcu_printf("Running classification for model %d\n", model_number);

  // Start timer0 counting down from UINT32_MAX.
  timer0_en_write(0);
  timer0_reload_write(0);
  timer0_load_write(UINT32_MAX);
  timer0_en_write(1);

  int8_t score;
  tflite_classify(model_number, &score);

  // Report how long the classification took.
  timer0_update_value_write(1);
  uint32_t t = timer0_value_read();
  if (t == 0) {
    mcu_printf("Classification took longer than %u cycles (%ums)\n", UINT32_MAX,
               UINT32_MAX / (CONFIG_CLOCK_FREQUENCY / 1000));
  } else {
    uint32_t elapsed_cycles = UINT32_MAX - t;
    mcu_printf("Classification took %u cycles (%ums)\n", elapsed_cycles,
               elapsed_cycles / (CONFIG_CLOCK_FREQUENCY / 1000));
  }

  mcu_printf("Classification model %d result: %d\n", model_number, score);
  return score;
}

uint16_t calculate_feature(bool rotate_image, int model_number) {
  mcu_printf("Capturing image for model %d\n", model_number);
  capture(rotate_image, tflite_get_input(model_number));
  // The image entropy value for unusable images is much greater
  // than for usable. Values above the threshold signal unusability.
  uint32_t entropy = calculate_entropy(tflite_get_input(model_number));
  if (entropy >= ENTROPY_THRESHOLD) {
    return 0;
  }
  return 0x8000 | (0xff & (uint8_t)run_classify(model_number));
}

static void model_selftest(int model_number) {
  mcu_printf("Self test model %d\n", model_number);
  int8_t* input = tflite_get_input(model_number);
  for (size_t i = 0; i < cat_picture_len; i++) {
    input[i] = (int)cat_picture[i] - 128;
  }
  int8_t score = run_classify(model_number);
  const int8_t expected_score = model_number == 1 ? -77 : -126;
  if (score != expected_score)
    printf("Model self-test FAILED: expected %d, actual %d\n", expected_score,
           score);
}

// Reads configuration registers from the MCU
static void read_mcu_config(struct configuration* config) {
  puts("Reading MCU configuration");
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_READ_CONFIG_REGISTERS;
  mcu_transaction();
  memcpy(config, &mcu_buf_in[2], sizeof(struct configuration));
}

// Sends a poll command to the MCU. If a packet was previously lost and the MCU
// is waiting to send us a response that we're not expecting, this will get us
// back in sync.
static void poll_mcu(void) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_POLL;
  mcu_transaction();
}

static enum debug_command get_debug_command(uint16_t* arg_out) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_GET_DEBUG_CMD;
  mcu_transaction();

  if (mcu_buf_in[0] == MCU_RESPONSE_OK &&
      mcu_buf_in[1] == MCU_COMMAND_GET_DEBUG_CMD) {
    *arg_out = (mcu_buf_in[4] << 8) | mcu_buf_in[3];
    return (enum debug_command)mcu_buf_in[2];
  }
  return DEBUG_CMD_NONE;
}

// Sets the state of the MCU debug LED
static void set_mcu_led(uint8_t ledstate) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_SET_DEBUG_LED;
  mcu_buf_out[1] = ledstate;
  mcu_transaction();
  if (mcu_buf_in[0] != MCU_RESPONSE_OK ||
      mcu_buf_in[1] != MCU_COMMAND_SET_DEBUG_LED) {
    puts("MCU did not acknowledge SET_DEBUG_LED command");
  }
}

// Randomly generated sequences that identify the start and end of an image.
// These sequences are recognized by hps-mon.
const uint8_t kStartImage[] = {13, 174, 212, 250, 191, 30, 138, 125};
const uint8_t kEndImage[] = {227, 114, 212, 105, 101, 111, 21, 193};

static void transfer_image(const uint8_t* data) {
  struct mcu_stream stream;
  mcu_stream_initialize(&stream, MCU_COMMAND_SEND_IMAGE);
  size_t image_size = CAM_IMAGE_WIDTH * CAM_IMAGE_HEIGHT;
  mcu_stream_send_data(&stream, kStartImage, sizeof(kStartImage));
  mcu_stream_send_data(&stream, data, image_size);
  mcu_stream_send_data(&stream, kEndImage, sizeof(kEndImage));
  mcu_stream_flush(&stream);
}

void tflite_error_callback(const char* format, va_list args) {
  mcu_vprintf(format, args);
  mcu_printf("\n");
}

void report_boot(void) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_REPORT_BOOT;
  mcu_buf_out[1] = SOC_ROM_VERSION;
  mcu_transaction();
}

void init(void) {
  irq_setmask(0);
  irq_setie(1);
  spi_init();

  mcu_printf("HPS SoC application " HPS_VERSION);
  mcu_printf(" (built " HPS_BUILD_DATE);
  mcu_printf(" by " HPS_BUILD_USER ")\n");

  report_boot();

  // Initialize TFLite
  tflite_init(tflite_error_callback);

  // Test evaluation
  model_selftest(1);
  model_selftest(2);

  // Calibrate autoexposure
  write_ae_cfg();
}

struct {
  bool debug_run_classify;
  bool debug_run_calibration;
  bool debug_transfer_image_to_host;
  bool rotate_image;
} application_state = {false, false, false, true};

void read_all_debug_commands(void) {
  // Fetch and process debug commands until there aren't any more.
  while (true) {
    uint16_t cmd_arg = 0;
    switch (get_debug_command(&cmd_arg)) {
      case DEBUG_CMD_CLASSIFY_OFF:
        mcu_printf("Image classification disabled\n");
        application_state.debug_run_classify = false;
        break;
      case DEBUG_CMD_CLASSIFY_ON:
        mcu_printf("Image classification enabled\n");
        application_state.debug_run_classify = true;
        break;
      case DEBUG_CMD_TRANSFER_IMAGE_OFF:
        mcu_printf("Image transfer disabled\n");
        application_state.debug_transfer_image_to_host = false;
        break;
      case DEBUG_CMD_TRANSFER_IMAGE_ON:
        mcu_printf("Image transfer enabled\n");
        application_state.debug_transfer_image_to_host = true;
        break;
      case DEBUG_CMD_RUN_EXPOSURE_CALIBRATION:
        application_state.debug_run_calibration = true;
        break;
      case DEBUG_CMD_ROTATE_IMAGE_CLOCKWISE_OFF:
        mcu_printf("Image rotation clockwise disabled\n");
        application_state.rotate_image = false;
        break;
      case DEBUG_CMD_ROTATE_IMAGE_CLOCKWISE_ON:
        mcu_printf("Image rotation clockwise enabled\n");
        application_state.rotate_image = true;
        break;
      case DEBUG_CMD_NONE:
        return;
    }
  }
}

int main(void) {
  init();

  struct configuration configuration = {0};

  uint32_t loop_count = 0;
  while (1) {
    loop_count++;
    poll_mcu();

    // Request commands from host
    read_all_debug_commands();
    read_mcu_config(&configuration);

    if (application_state.debug_run_calibration || calibration_required()) {
      calibrate(CALIBRATION_FRAMES);
      mcu_printf("Camera calibration completed\n");
      application_state.debug_run_calibration = false;
    }
    if (application_state.debug_transfer_image_to_host ||
        configuration.enabled_features & 0x80) {
      mcu_printf("Capturing image\n");
      capture(application_state.rotate_image, tflite_get_input(1));
      transfer_image((const uint8_t*)tflite_get_input(1));
    }

    // Run configured inferences
    struct status inference_status = {0, configuration.enabled_features,
                                      loop_count, 0};
    inference_status.second_person_status = 0;

    write_ae_cfg();  // TODO(avg): is this required?
    if (application_state.debug_run_classify ||
        configuration.enabled_features & 1) {
      inference_status.person_status =
          calculate_feature(application_state.rotate_image, 1);
    }
    if (application_state.debug_run_classify ||
        configuration.enabled_features & 2) {
      inference_status.person_status =
          calculate_feature(application_state.rotate_image, 2);
    }

    write_mcu_status(&inference_status);
    set_mcu_led(loop_count % 2);
  }
}
