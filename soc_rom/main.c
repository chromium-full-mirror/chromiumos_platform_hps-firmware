// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <SparkFun_HM01B0_Camera_ArduinoLibrary/src/hm01b0.h>
#include <console.h>
#include <generated/csr.h>
#include <hps_i2c.h>
#include <hw/common.h>
#include <irq.h>
#include <stdio.h>
#include <string.h>

#include "camera.h"
#include "cat_picture.h"
#include "generated/csr.h"
#include "hm01b0_platform.h"
#include "hps_version.h"
#include "mcu.h"
#include "litex_shim/shim.h"
#include "transpose.h"

#define CAM_CHECK(status, msg)     \
  {                                \
    if (status != HM01B0_ERR_OK) { \
      mcu_printf(msg " failed\n"); \
    }                              \
  }

static hm01b0_ae_cfg_t aecfg;
static uint8_t ae_target = 100;
static hm01b0_snr_expo_gain_ctrl_t expo_gain_ctrl;
static bool automatic_exposure_enabled = true;
static int recalibration_threshold = 0;

// This version is exposed via an I2C register and has no semantic meaning. It's
// bumped whenever a developer decides to bump it and is just for development
// and debugging purposes. Maximum supported value is currently 255, although
// expanding that to a 16 bit value is trivial.
#define SOC_ROM_VERSION 1

#define CALIBRATION_FRAMES 15

#define CAM_IMAGE_WIDTH 320
#define CAM_IMAGE_HEIGHT 240

// Images with entropy below the threshold are usable.
// The entropy threshold was defined empirically.
// For more details, please refer to b/204518775#comment9.
#define ENTROPY_THRESHOLD 270000000

struct configuration {
  uint16_t enabled_features;
};

struct status {
  int8_t score;
  uint16_t enabled_features;
  uint16_t loop_count;
};

// Wait for DMA to finish
static void cam_dma_wait(void) {
  while (cam_control_running_read()) {
    /* wait */;
  }
}

// Start camera DMA
static void cam_dma_enable(uint16_t column_start,
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
  cam_control_base_address_write((uint32_t)tflite_get_input());

  cam_control_start_run_write(1);
}

static void capture(bool rotate_image) {
  hm01b0_status_e status;
  if (!automatic_exposure_enabled) {
    status = hm01b0_restore_exposure_gains(&hm01b0_cfg, &expo_gain_ctrl);
    CAM_CHECK(status, "restore_exposure");
  }
  status = hm01b0_cmd_update(&hm01b0_cfg);
  CAM_CHECK(status, "status");

  // Command camera to take a frame
  wait_for_camera_standby();
  if (rotate_image) {
    // Clips 240x320 image from X = [42, 282], Y = [2, 322].
    cam_dma_enable((324 - CAM_IMAGE_HEIGHT) / 2, CAM_IMAGE_HEIGHT, 2,
                   CAM_IMAGE_WIDTH);
  } else {
    // Clips 320x240 image from X = [2, 322], Y = [42, 282].
    cam_dma_enable(2, CAM_IMAGE_WIDTH, (324 - CAM_IMAGE_HEIGHT) / 2,
                   CAM_IMAGE_HEIGHT);
  }
  set_camera_streaming_mode(1);
  cam_dma_wait();
  if (rotate_image) {
    // Rotates 240x320 image to an expected 320x240 format.
    rotate_240_320_clockwise((uint8_t*)tflite_get_input());
  }
}

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

// Fetches a pixel from the TFLite input vector location.
//
// Since values are stored in the TFLite vector as signed ints, this function
// adds 128 to recreate the original unsigned value.
inline uint8_t get_pixel(size_t x, size_t y) {
  size_t index = CAM_IMAGE_WIDTH * y + x;
  return 128 + tflite_get_input()[index];
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
// Assumes input data vector is already filled. Writes the result of the
// classification to `status`.
void run_classify(struct status* status) {
  mcu_printf("Running classification\n");

  // Start timer0 counting down from UINT32_MAX.
  timer0_en_write(0);
  timer0_reload_write(0);
  timer0_load_write(UINT32_MAX);
  timer0_en_write(1);

  tflite_classify(&status->score);

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

  mcu_printf("Classification result: %d\n", status->score);
}

static void model_selftest(void) {
  struct status selftest_status = {0};
  const int8_t expected_score = -116;
  int8_t* input = tflite_get_input();
  for (size_t i = 0; i < cat_picture_len; i++) {
    input[i] = (int)cat_picture[i] - 128;
  }
  run_classify(&selftest_status);
  if (selftest_status.score != expected_score)
    printf("Model self-test FAILED: expected %d, actual %d\n", expected_score,
           selftest_status.score);
}

// Initializes the camera
static void camera_init() {
  hm01b0_status_e status;
  puts("Camera: Normal mode init");
  hm01b0_init_system(&hm01b0_cfg, (hm_script_t*)hps_init,
                     sizeof(hps_init) / sizeof(hm_script_t));
  status = hm01b0_save_exposure_gains(&hm01b0_cfg, &expo_gain_ctrl);
  CAM_CHECK(status, "save_exposure");
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

static void transfer_image(void) {
  struct mcu_stream stream;
  mcu_stream_initialize(&stream, MCU_COMMAND_SEND_IMAGE);
  size_t image_size = CAM_IMAGE_WIDTH * CAM_IMAGE_HEIGHT;
  mcu_stream_send_data(&stream, kStartImage, sizeof(kStartImage));
  mcu_stream_send_data(&stream, (const uint8_t*)tflite_get_input(), image_size);
  mcu_stream_send_data(&stream, kEndImage, sizeof(kEndImage));
  mcu_stream_flush(&stream);
}

static void set_camera_register(uint16_t address, uint8_t value) {
  if (address == 0) {
    mcu_printf(
        "Attempt to set custom register value without first setting address");
    return;
  }
  uint32_t ui32Err =
      hm01b0_write_reg(&hm01b0_cfg, address, (uint8_t*)&value, sizeof(value));
  if (ui32Err != HM01B0_ERR_OK) {
    mcu_printf("Failed to set camera register 0x%02x to 0x%02x\n", address,
               value);
  } else {
    mcu_printf("Set camera register 0x%02x to 0x%02x\n", address, value);
  }
}

// Calculates an approximation of ln(x)/ln(2) * 2^24.
//
// Accurately calculates the integer portion of ln(x)/ln(2), then relies on
// the approximation ln(x) ~= (x-1) to calculate the fractional part of the base
// 2 logarithm. Bit shifting is used to pack integer and fraction into a 32 bit
// result that is a fixed point number, with the integer part in the top 8 bits
// and fractional part in the lower 24 bits.
// The resulting output is accurate to within 6%, with error (generally)
// decreasing for larger x."
uint32_t fixed_log2(uint32_t x) {
  uint32_t shift = 24;
  uint32_t exp = 0;
  while (x >= (uint32_t)(1 << (exp + 1))) {
    exp += 1;
  }
  uint32_t frac = x - (1 << exp);
  return (exp << shift) + (frac << (shift - exp));
}

// This function calculates if the most recently taken image is usable.
// Images which contain most of the pixels in a small number of buckets
// will have much greater entropy value.
uint32_t image_entropy() {
  uint32_t histogram[256] = {0};
  for (int x = 0; x < CAM_IMAGE_WIDTH; x++) {
    for (int y = 0; y < CAM_IMAGE_HEIGHT; y++) {
      histogram[get_pixel(x, y)]++;
    }
  }
  // This method of calculating entropy is very custom and it has been
  // achieved and tested using Rust adequate.
  // For more details, please refer to b/204518775#comment9.
  uint64_t entropy = 0;
  for (int i = 0; i < 256; i++) {
    if (histogram[i] == 0) {
      continue;
    }
    uint64_t value = histogram[i];
    // To preserve some of the fraction value, smaller shift is applied.
    entropy += ((value * fixed_log2(value)) >> 16);
  }
  return entropy;
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

int main(void) {
  irq_setmask(0);
  irq_setie(1);
  spi_init();

  mcu_printf("HPS SoC application " HPS_VERSION);
  mcu_printf(" (built " HPS_BUILD_DATE);
  mcu_printf(" by " HPS_BUILD_USER ")\n");

  report_boot();

  hm01b0_status_e status;

  // init camera
  status = hm01b0_mclk_enable(&hm01b0_cfg);
  CAM_CHECK(status, "mclk");
  status = hm01b0_init_if(&hm01b0_cfg);
  CAM_CHECK(status, "init");

  // Auto exposure struct
  memset((void*)&aecfg, 0x00, sizeof(hm01b0_ae_cfg_t));

  // Load up init script
  camera_init();

  // Initialize TFLite
  tflite_init(tflite_error_callback);

  // Perform model self-test
  model_selftest();

  // Calibrate autoexposure
  write_ae_cfg();
  int calibration_frames = CALIBRATION_FRAMES;

  struct configuration configuration = {0};

  uint32_t loop_count = 0;
  bool do_run_classify = false;
  bool do_run_exposure_calibration = false;
  bool do_transfer_image_to_host = false;
  bool rotate_image = true;
  uint16_t custom_register_address = 0;
  while (1) {
    loop_count++;
    poll_mcu();
    // Fetch and process debug commands until there aren't any more.
    bool should_get_command = true;
    while (should_get_command) {
      uint16_t cmd_arg = 0;
      switch (get_debug_command(&cmd_arg)) {
        case DEBUG_CMD_CLASSIFY_OFF:
          mcu_printf("Image classification disabled\n");
          do_run_classify = false;
          break;
        case DEBUG_CMD_CLASSIFY_ON:
          mcu_printf("Image classification enabled\n");
          do_run_classify = true;
          break;
        case DEBUG_CMD_TRANSFER_IMAGE_OFF:
          mcu_printf("Image transfer disabled\n");
          do_transfer_image_to_host = false;
          break;
        case DEBUG_CMD_TRANSFER_IMAGE_ON:
          mcu_printf("Image transfer enabled\n");
          do_transfer_image_to_host = true;
          break;
        case DEBUG_CMD_RUN_EXPOSURE_CALIBRATION:
          do_run_exposure_calibration = true;
          break;
        case DEBUG_CMD_SET_CUSTOM_CAM_REG_ADDRESS:
          custom_register_address = cmd_arg;
          break;
        case DEBUG_CMD_SET_CUSTOM_CAM_REG_VALUE:
          set_camera_register(custom_register_address, cmd_arg);
          break;
        case DEBUG_CMD_ROTATE_IMAGE_CLOCKWISE_OFF:
          mcu_printf("Image rotation clockwise disabled\n");
          rotate_image = false;
          break;
        case DEBUG_CMD_ROTATE_IMAGE_CLOCKWISE_ON:
          mcu_printf("Image rotation clockwise enabled\n");
          rotate_image = true;
          break;
        case DEBUG_CMD_NONE:
          should_get_command = false;
          break;
      }
    }
    // Run configured functions
    read_mcu_config(&configuration);
    struct status inference_status = {0};
    inference_status.enabled_features = configuration.enabled_features;
    inference_status.loop_count = loop_count;

    write_ae_cfg();
    if (do_run_exposure_calibration) {
      calibration_frames = CALIBRATION_FRAMES;
      do_run_exposure_calibration = false;
    }
    if (calibration_frames > 0) {
      if (automatic_exposure_enabled) {
        calibrate(calibration_frames);
        mcu_printf("Camera calibration completed\n");
      } else {
        mcu_printf("Skipping calibration since AE is disabled\n");
      }
      calibration_frames = 0;
    }
    mcu_printf("Capturing image\n");
    capture(rotate_image);
    uint32_t entropy = image_entropy();
    if (entropy >= ENTROPY_THRESHOLD) {
      // TODO(b/204518775): Notify hpsd about unusable picture.
      mcu_printf("Image unusable (entropy: %d)\n", entropy);
    } else {
      mcu_printf("Image usable (entropy: %d)\n", entropy);
    }
    if (calibration_required()) {
      calibrate(CALIBRATION_FRAMES);
      mcu_printf("Automatic re-calibration completed\n");
    }
    if (do_transfer_image_to_host || configuration.enabled_features & 0x80) {
      transfer_image();
    }
    // We run classification if it's enabled via HPS-mon or if feature 0 is
    // enabled from the MCU via I2C.
    if (do_run_classify || ((configuration.enabled_features & 1) != 0)) {
      run_classify(&inference_status);
    }
    write_mcu_status(&inference_status);
    set_mcu_led(loop_count % 2);
  }
}
