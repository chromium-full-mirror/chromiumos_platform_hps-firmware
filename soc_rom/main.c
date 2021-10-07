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
#include <uart.h>

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

#define HM01B0_REG_AE_CTRL (0x2100)

#define CAM_IMAGE_WIDTH 320
#define CAM_IMAGE_HEIGHT 240

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
  status =
      hm01b0_set_mode(&hm01b0_cfg, HM01B0_REG_MODE_SELECT_STREAMING_NFRAMES, 1);
  CAM_CHECK(status, "set_mode");
  cam_dma_wait();
  if (rotate_image) {
    // Rotates 240x320 image to an expected 320x240 format.
    rotate_240_320_clockwise((uint8_t*)tflite_get_input());
  }
}

static void print_ae_cfg(void) {
  hm01b0_status_e status;
  status = hm01b0_get_ae(&hm01b0_cfg, &aecfg);
  if (status == HM01B0_ERR_AE_NOT_CONVERGED) {
    mcu_printf("AE not converged.\n");
  } else {
    CAM_CHECK(status, "get_ae");
  }

  mcu_printf("target mean = %d\n", aecfg.ui8AETargetMean);
  mcu_printf("min mean    = %d\n", aecfg.ui8AEMinMean);
  mcu_printf("conv in th  = %d\n", aecfg.ui8ConvergeInTh);
  mcu_printf("conv out th = %d\n", aecfg.ui8ConvergeOutTh);
  mcu_printf("mean        = %d\n", aecfg.ui8AEMean);
}

// Write the target auto exposure into the camera
void write_ae_cfg(void) {
  static int last_target = -1;
  if (last_target != ae_target) {
    hm01b0_write_reg(&hm01b0_cfg, HM01B0_REG_AE_TARGET_MEAN, &ae_target, 1);
    last_target = ae_target;
  }
}

// Stream nframes to allow calibration
static void calibrate(int nframes) {
  wait_for_camera_standby();
  mcu_printf("Calibrating with %d frames\n", nframes);
  hm01b0_status_e status = hm01b0_set_mode(
      &hm01b0_cfg, HM01B0_REG_MODE_SELECT_STREAMING_NFRAMES, nframes);
  CAM_CHECK(status, "set_mode");
  wait_for_camera_standby();
  print_ae_cfg();
}

// Recalibration is required when AE mean is far from the AE target
static int calibration_required(void) {
  if (recalibration_threshold < 5) {
    recalibration_threshold++;
  }
  hm01b0_get_ae(&hm01b0_cfg, &aecfg);
  // If the autoexposure mean value is 20 steps away from the target there was
  // a major light change in the environment and the recalibration is required.
  // Threshold makes sure that we don't trigger recalibration on every single
  // cycle. Both values are set based on the experiments.
  if (abs(ae_target - aecfg.ui8AEMean) > 20 && recalibration_threshold == 5) {
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

// Set the auto exposure target variable, based on user onput
static bool user_set_ae_target(int input, int key, int delta) {
  if (input != key) {
    return false;
  }
  ae_target += delta;
  printf("Set AE target to %d\n", ae_target);
  return true;
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

// Allows the user to toggle a flag that indicates which actions to include in
// the main loop.
static bool user_toggle(int input, int key, const char* desc, bool* var) {
  if (input != key) {
    return false;
  }
  *var = !*var;
  printf("%s now %s\n", desc, *var ? "ON" : "OFF");
  return true;
}

static void print_expo_gain(void) {
  mcu_printf("Analog gain: 0x%02x\n", (int)expo_gain_ctrl.ui8AGain);
  mcu_printf("Integration: 0x%02x\n", ((int)expo_gain_ctrl.ui8IntegrationH)
                                              << 8 |
                                          (int)expo_gain_ctrl.ui8IntegrationL);
  mcu_printf("Digital gain: 0x%02x\n", ((int)expo_gain_ctrl.ui8DGain_H) << 8 |
                                           (int)expo_gain_ctrl.ui8DGain_L);
}

static void read_and_print_expo_gain(void) {
  hm01b0_snr_expo_gain_ctrl_t gains;
  hm01b0_status_e status;
  status = hm01b0_save_exposure_gains(&hm01b0_cfg, &gains);
  if (status != HM01B0_ERR_OK) {
    mcu_printf("Failed to read exposure gains\n");
    return;
  }
  print_expo_gain();
}

static void set_automatic_exposure_target(uint8_t value) {
  if (value == 0) {
    uint32_t ui32Err = hm01b0_write_reg(&hm01b0_cfg, HM01B0_REG_AE_CTRL,
                                        (uint8_t*)&value, sizeof(value));
    if (ui32Err == HM01B0_ERR_OK) {
      automatic_exposure_enabled = false;
      mcu_printf("Disabled automatic exposure\n");
    } else {
      mcu_printf("Failed to disable automatic exposure\n");
    }
  } else {
    if (!automatic_exposure_enabled) {
      uint8_t enable = 1;
      uint32_t ui32Err = hm01b0_write_reg(&hm01b0_cfg, HM01B0_REG_AE_CTRL,
                                          (uint8_t*)&enable, sizeof(enable));
      if (ui32Err == HM01B0_ERR_OK) {
        automatic_exposure_enabled = true;
        mcu_printf("Enabled automatic exposure\n");
      } else {
        mcu_printf("Failed to enable automatic exposure\n");
      }
    }

    uint32_t ui32Err = hm01b0_write_reg(&hm01b0_cfg, HM01B0_REG_AE_TARGET_MEAN,
                                        (uint8_t*)&value, sizeof(value));
    if (ui32Err == HM01B0_ERR_OK) {
      mcu_printf("Set automatic exposure target to 0x%02x\n", value);
    } else {
      mcu_printf("Failed to disable automatic exposure\n");
    }
  }
}

static void set_analog_gain(uint16_t value) {
  if (value != 0 && value != 0x10 && value != 0x20 && value != 0x30) {
    mcu_printf("Invalid analog gain. Valid values are 0, 0x10, 0x20 or 0x30\n");
  } else {
    mcu_printf("Setting analog gain to 0x%02x\n", value);
    expo_gain_ctrl.ui8AGain = value;
    print_expo_gain();
  }
}

static void set_digital_gain(uint16_t value) {
  mcu_printf("Setting digital gain to 0x%02x\n", value);
  expo_gain_ctrl.ui8DGain_H = value >> 8;
  expo_gain_ctrl.ui8DGain_L = value & 0xff;
  print_expo_gain();
}

static void set_integration(uint16_t value) {
  mcu_printf("Setting integration to 0x%02x\n", value);
  expo_gain_ctrl.ui8IntegrationH = value >> 8;
  expo_gain_ctrl.ui8IntegrationL = value & 0xff;
  print_expo_gain();
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
  uart_init();
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
  bool do_show_auto_exposure = false;
  bool do_run_exposure_calibration = false;
  bool do_transfer_image_to_host = false;
  bool rotate_image = true;
  uint16_t custom_register_address = 0;
  while (1) {
    // Accept input to configure which functions are run
    printf("[%u] Press key (s, r, c, a, + , -, e, t)\n", loop_count++);
    if (uart_read_nonblock()) {
      int c = uart_read();
      bool recognized = false;
      recognized |= user_toggle(c, 'c', "Run classifier", &do_run_classify);
      recognized |= user_toggle(c, 't', "Transfer image to host",
                                &do_transfer_image_to_host);
      recognized |=
          user_toggle(c, 'a', "Show auto exposure", &do_show_auto_exposure);
      recognized |= user_set_ae_target(c, '+', +10);
      recognized |= user_set_ae_target(c, '-', -10);
      recognized |= user_toggle(c, 'e', "Run Exposure calibration",
                                &do_run_exposure_calibration);
      if (!recognized) {
        printf("Did not understand input \"%c\"\n", c);
      }
    }
    poll_mcu();
    // Fetch and process debug commands until there aren't any more.
    bool should_get_command = true;
    // Buffer to store camera register addresses and values.
    uint16_t camera_addresses[10];
    uint8_t camera_values[10];
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
        case DEBUG_CMD_SET_ANALOG_GAIN:
          set_analog_gain(cmd_arg);
          break;
        case DEBUG_CMD_SET_DIGITAL_GAIN:
          set_digital_gain(cmd_arg);
          break;
        case DEBUG_CMD_SET_INTEGRATION:
          set_integration(cmd_arg);
          break;
        case DEBUG_CMD_SET_CUSTOM_CAM_REG_ADDRESS:
          custom_register_address = cmd_arg;
          break;
        case DEBUG_CMD_SET_CUSTOM_CAM_REG_VALUE:
          set_camera_register(custom_register_address, cmd_arg);
          break;
        case DEBUG_CMD_READ_CAM_REG:
          print_camera_register(cmd_arg);
          break;
        case DEBUG_CMD_PRINT_CAM_GAINS:
          read_and_print_expo_gain();
          break;
        case DEBUG_CMD_SET_AUTOMATIC_EXPOSURE_TARGET:
          set_automatic_exposure_target(cmd_arg);
          break;
        case DEBUG_CMD_PRINT_AE_CFG:
          print_ae_cfg();
          break;
        case DEBUG_CMD_READ_CAMERA_I2C:
          // TODO(jakubm): Delete static alloc in the future changes.
          camera_addresses[0] = 0x0000;
          camera_addresses[1] = 0x0001;
          read_camera_i2c(camera_addresses, 2, camera_values);
          mcu_printf("0x%02x%02x\n", camera_values[0], camera_values[1]);
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
    if (calibration_required()) {
      calibrate(CALIBRATION_FRAMES);
      mcu_printf("Automatic re-calibration completed\n");
    }
    if (do_transfer_image_to_host) {
      transfer_image();
    }
    if (do_show_auto_exposure) {
      print_ae_cfg();
    }
    // We run classification if it's enabled via the UART, or if feature 0 is
    // enabled from the MCU via I2C.
    if (do_run_classify || ((configuration.enabled_features & 1) != 0)) {
      run_classify(&inference_status);
    }
    write_mcu_status(&inference_status);
    set_mcu_led(loop_count % 2);
  }
}
