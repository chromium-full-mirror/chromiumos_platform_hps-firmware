// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <SparkFun_HM01B0_Camera_ArduinoLibrary/src/hm01b0.h>
#include <SparkFun_HM01B0_Camera_ArduinoLibrary/src/hm01b0_raw8_qvga_8bits_lsb_5fps.h>
#include <console.h>
#include <generated/csr.h>
#include <hps_i2c.h>
#include <hw/common.h>
#include <irq.h>
#include <stdio.h>
#include <string.h>
#include <uart.h>

#include "hm01b0_platform.h"
#include "hps_version.h"
#include "mcu.h"
#include "litex_shim/shim.h"

#define CAM_CHECK(status, msg)     \
  {                                \
    if (status != HM01B0_ERR_OK) { \
      puts(msg " failed\n");       \
    }                              \
  }

static hm01b0_ae_cfg_t aecfg;
static uint8_t ae_target = 100;

#define FB_SIZE ((size_t)HM01B0_PIXEL_X_NUM * (size_t)HM01B0_PIXEL_Y_NUM)
uint8_t frame_buffer[FB_SIZE];

// Start camera DMA
static void cam_dma_enable(void) {
  cam_control_dma_base_write((uint32_t)frame_buffer);
  cam_control_dma_length_write(HM01B0_PIXEL_Y_NUM * HM01B0_PIXEL_X_NUM);
  cam_control_w_dma_signed_write(1);
  cam_control_dma_enable_write(1);
}

// Wait for DMA to finish, then turn it off
static void cam_dma_wait(void) {
  while (!cam_control_dma_done_read())
    /* wait */;
  cam_control_dma_enable_write(0);
}

static void capture(void) {
  hm01b0_status_e status;
  status = hm01b0_cmd_update(&hm01b0_cfg);
  CAM_CHECK(status, "status");

  // Command camera to take a frame
  cam_dma_enable();
  status =
      hm01b0_set_mode(&hm01b0_cfg, HM01B0_REG_MODE_SELECT_STREAMING_NFRAMES, 1);
  CAM_CHECK(status, "set_mode");
  cam_dma_wait();
}

static void print_ae_cfg(void) {
  hm01b0_status_e status;
  status = hm01b0_get_ae(&hm01b0_cfg, &aecfg);
  if (status == HM01B0_ERR_AE_NOT_CONVERGED) {
    puts("AE not converged.\n");
  } else {
    CAM_CHECK(status, "get_ae");
  }

  printf("target mean = %d\n", aecfg.ui8AETargetMean);
  printf("min mean    = %d\n", aecfg.ui8AEMinMean);
  printf("conv in th  = %d\n", aecfg.ui8ConvergeInTh);
  printf("conv out th = %d\n", aecfg.ui8ConvergeOutTh);
  printf("mean        = %d\n", aecfg.ui8AEMean);
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
  printf("Calibrating with %d frames\n", nframes);
  hm01b0_set_mode(&hm01b0_cfg, HM01B0_REG_MODE_SELECT_STREAMING_NFRAMES,
                  nframes);
  for (int i = 0; i < nframes; i++) {
    cam_dma_enable();
    printf("  - frame %d\n", i + 1);
    cam_dma_wait();
  }
  print_ae_cfg();
  hm01b0_set_mode(&hm01b0_cfg, HM01B0_REG_MODE_SELECT_STANDBY, 0);
}

// Fetches a pixel from the TFLite input vector location.
//
// Since values are stored in the TFLite vector as signed ints, this function
// adds 128 to recreate the original unsigned value.
inline uint8_t get_pixel(size_t x, size_t y) {
  size_t index = HM01B0_PIXEL_X_NUM * y + x;
  return 128 + frame_buffer[index];
}

// Looks up a conversion from a 6 bit integer and a base 64 character
static inline char b64_lookup(uint8_t n) {
  static const char* b64_table =
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  return b64_table[0x3f & n];
}
#define b64_line_len (HM01B0_PIXEL_X_NUM / 3 * 4 + 1)
static char b64_buf[b64_line_len + 1];

// Outputs a base64 representation of each of line the camera image.
void show_raw_frame(void) {
  puts("+++ base64 +++");
  b64_buf[b64_line_len] = 0;
  for (size_t y = 0; y < HM01B0_PIXEL_Y_NUM; y++) {
    size_t j = 0;
    for (size_t x = 0; x < HM01B0_PIXEL_X_NUM; x += 3) {
      uint8_t p0 = get_pixel(x + 0, y);
      uint8_t p1 = get_pixel(x + 1, y);
      uint8_t p2 = get_pixel(x + 2, y);
      b64_buf[j++] = b64_lookup(p0 >> 2);
      b64_buf[j++] = b64_lookup((p0 << 4) | (0xf & (p1 >> 4)));
      b64_buf[j++] = b64_lookup((p1 << 2) | (0x3 & (p2 >> 6)));
      b64_buf[j++] = b64_lookup(p2);
    }
    puts(b64_buf);
  }
  puts("--- base64 ---");
}

// Sends status register content to the mcu
static void write_mcu_status(int8_t person_score, int8_t not_person_score) {
  puts("Updating status in MCU");
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_WRITE_STATUS_REGISTERS;
  mcu_buf_out[1] = person_score;
  mcu_buf_out[2] = not_person_score;
  mcu_transaction();
  if (mcu_buf_in[0] != MCU_RESPONSE_OK ||
      mcu_buf_in[1] != MCU_COMMAND_WRITE_STATUS_REGISTERS) {
    puts("MCU did not acknowledge command");
  }
}

// Extracts a 96x96 square from the frame buffer into the TfLM input
//
// It does this by averaging 2x2 pixel groups over a 192x192 area in the center
// of the frame.
void extract_input_from_frame_buffer() {
  puts("extract start");
  int8_t* input = tflite_get_input();
  // Resize to 96x96 by copying a 192*192 square from the middle
  for (size_t in_y = 0; in_y < 96; in_y++) {
    size_t tl_y = (HM01B0_PIXEL_Y_NUM - 96 * 2) / 2 + 2 * in_y;
    for (size_t in_x = 0; in_x < 96; in_x++) {
      // average 2x2 sqaure
      // Calculate top left frame x, y positions
      size_t tl_x = (HM01B0_PIXEL_X_NUM - 96 * 2) / 2 + 2 * in_x;
      int val = 0;
      val += get_pixel(tl_x + 0, tl_y + 0);
      val += get_pixel(tl_x + 1, tl_y + 0);
      val += get_pixel(tl_x + 0, tl_y + 1);
      val += get_pixel(tl_x + 1, tl_y + 1);
      *input++ = ((val + 2) / 4);
    }
  }
  puts("extract end");
}

// Performs the classification.
//
// Assumes input data vector is already filled. Writes the result of the
// classification to the MCU.
void run_classify() {
  int8_t person, not_person;
  puts("Running classification.");

  // Start timer0 counting down from UINT32_MAX.
  timer0_en_write(0);
  timer0_reload_write(0);
  timer0_load_write(UINT32_MAX);
  timer0_en_write(1);

  tflite_classify(&person, &not_person);

  // Report how long the classification took.
  timer0_update_value_write(1);
  uint32_t t = timer0_value_read();
  if (t == 0) {
    printf("Classification took longer than %u cycles (%ums)\n", UINT32_MAX,
           UINT32_MAX / (CONFIG_CLOCK_FREQUENCY / 1000));
  } else {
    uint32_t elapsed_cycles = UINT32_MAX - t;
    printf("Classification took %u cycles (%ums)\n", elapsed_cycles,
           elapsed_cycles / (CONFIG_CLOCK_FREQUENCY / 1000));
  }

  printf("Person: %d, Not person %d\n", person, not_person);
  write_mcu_status(person, not_person);
}

// Initializes the camera
static void camera_init() {
  puts("Camera: Normal mode init");
  hm01b0_init_system(&hm01b0_cfg, (hm_script_t*)sHM01B0InitScript,
                     sizeof(sHM01B0InitScript) / sizeof(hm_script_t));
}

// Reads configuration registers from the MCU
static void read_mcu_config(void) {
  puts("Reading MCU configuration");
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_READ_CONFIG_REGISTERS;

  // Just running the transaction will print debug info
  mcu_transaction();
}

// Sends a poll command to the MCU. If a packet was previously lost and the MCU
// is waiting to send us a response that we're not expecting, this will get us
// back in sync.
static void poll_mcu(void) {
  memset(mcu_buf_out, 0, MCU_BUFFER_LEN);
  mcu_buf_out[0] = MCU_COMMAND_POLL;
  mcu_transaction();
}

// Sets the state of the MCU debug LED
static void set_mcu_led(uint8_t ledstate) {
  printf("Setting MCU debug LED %s\n", ledstate != 0 ? "on" : "off");
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

static void TransferImage(void) {
  McuStream stream;
  McuStreamInitialize(&stream, MCU_COMMAND_SEND_IMAGE);
  size_t image_size = HM01B0_PIXEL_X_NUM * HM01B0_PIXEL_Y_NUM;
  McuStreamSendData(&stream, kStartImage, sizeof(kStartImage));
  McuStreamSendData(&stream, (const uint8_t*)frame_buffer, image_size);
  McuStreamSendData(&stream, kEndImage, sizeof(kEndImage));
  McuStreamFlush(&stream);
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

int main(void) {
  irq_setmask(0);
  irq_setie(1);
  uart_init();

  mcu_printf("HPS SoC application " HPS_VERSION " (built " HPS_BUILD_DATE
             " by " HPS_BUILD_USER ")\n");

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
  tflite_init(model_pdti8);

  // Calibrate autoexposure
  write_ae_cfg();
  calibrate(15);

  mcu_printf("Camera calibration complete\n");

  uint32_t loop_counter = 0;
  bool do_spi_transfer = false;
  bool do_show_raw_frame = false;
  bool do_run_classify = true;
  bool do_show_auto_exposure = false;
  bool do_run_exposure_calibration = false;
  bool do_transfer_image_to_host = false;
  while (1) {
    // Accept input to configure which functions are run
    printf("[%u] Press key (s, r, c, a, + , -, e, t)\n", loop_counter++);
    if (uart_read_nonblock()) {
      int c = uart_read();
      bool recognized = false;
      recognized |= user_toggle(c, 's', "SPI Transfer", &do_spi_transfer);
      recognized |=
          user_toggle(c, 'r', "Raw frame display", &do_show_raw_frame);
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
    // Run configured functions
    if (do_spi_transfer) {
      read_mcu_config();
    }
    write_ae_cfg();
    if (do_run_exposure_calibration) {
      calibrate(5);
      do_run_exposure_calibration = false;
    }
    mcu_printf("Capturing image\n");
    capture();
    if (do_show_raw_frame) {
      show_raw_frame();
    }
    if (do_transfer_image_to_host) {
      TransferImage();
    }
    if (do_show_auto_exposure) {
      print_ae_cfg();
    }
    if (do_run_classify) {
      extract_input_from_frame_buffer();
      run_classify();
    }
    set_mcu_led(loop_counter % 2);
  }
}
