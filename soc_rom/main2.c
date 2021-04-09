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
#include "litex_shim/shim.h"

void isr(void) {
  unsigned int irqs;
  irqs = irq_pending() & irq_getmask();
  if (irqs & (1 << UART_INTERRUPT)) {
    uart_isr();
  }
}

#define CAM_CHECK(status, msg)     \
  {                                \
    if (status != HM01B0_ERR_OK) { \
      puts(msg " failed\n");       \
    }                              \
  }

#define FB_SIZE ((size_t)HM01B0_PIXEL_X_NUM * (size_t)HM01B0_PIXEL_Y_NUM)
uint8_t frame[FB_SIZE];

size_t x_fudge = 0;
inline uint8_t get_pixel(size_t x, size_t y) {
  size_t index = HM01B0_PIXEL_X_NUM * y + x;
  if (index >= x_fudge) {
    index -= x_fudge;
  }
  return frame[index];
}

const char shades[16] = {' ', '.', ',', '-', '+', '/', '1', 'r',
                         'n', 'x', 'L', '0', 'o', 'm', '*', '#'};

hm01b0_ae_cfg_t aecfg;

void print_ae_cfg(void) {
  printf("target mean = %d\n", aecfg.ui8AETargetMean);
  printf("min mean    = %d\n", aecfg.ui8AEMinMean);
  printf("conv in th  = %d\n", aecfg.ui8ConvergeInTh);
  printf("conv out th = %d\n", aecfg.ui8ConvergeOutTh);
  printf("mean        = %d\n", aecfg.ui8AEMean);
}

void calibrate() {
  puts("calibrating");
  hm01b0_status_e status;
  status = hm01b0_cal_ae(&hm01b0_cfg, 10, frame, sizeof(frame), &aecfg);
  CAM_CHECK(status, "cal_ae");

  status = hm01b0_get_ae(&hm01b0_cfg, &aecfg);
  if (status == HM01B0_ERR_AE_NOT_CONVERGED) {
    puts("AE not converged.\n");
  } else {
    CAM_CHECK(status, "get_ae");
  }
  print_ae_cfg();
}

// Load image from frame buffer into tflite input
void load_from_camera() {
  printf("start\n");
  int8_t* input = tflite_get_input();

  // Resize to 96x96 by copying a 192*192 square from the middle
  for (size_t in_y = 0; in_y < 96; in_y++) {
    size_t tl_y = 26 + 2 * in_y;
    for (size_t in_x = 0; in_x < 96; in_x++) {
      // average 2x2 sqaure
      // Calculate top left frame x, y positions
      size_t tl_x = 64 + 2 * in_x;
      int val = 0;
      val += get_pixel(tl_x + 0, tl_y + 0);
      val += get_pixel(tl_x + 1, tl_y + 0);
      val += get_pixel(tl_x + 0, tl_y + 1);
      val += get_pixel(tl_x + 1, tl_y + 1);

      *input++ = ((val + 2) / 4) - 128;
    }
  }
  printf("end\n");
}

void capture() {
  puts("capture");
  hm01b0_status_e status;

  status = hm01b0_cmd_update(&hm01b0_cfg);
  CAM_CHECK(status, "status");

  status =
      hm01b0_set_mode(&hm01b0_cfg, HM01B0_REG_MODE_SELECT_STREAMING_NFRAMES, 1);
  CAM_CHECK(status, "set_mode");

  status = hm01b0_blocking_read_oneframe(&hm01b0_cfg, frame, sizeof(frame));
  CAM_CHECK(status, "blocking_read_oneframe");

  load_from_camera();
}

void show_input() {
  int8_t* input = tflite_get_input();
  for (size_t in_y = 0; in_y < 96; in_y++) {
    char line[97];
    line[96] = 0;

    for (size_t in_x = 0; in_x < 96; in_x++) {
      int val = *input++;
      line[in_x] = shades[16 - (val + 128 + 8) / 16];
    }
    printf("%s\n", line);
  }
}

void dump() {
  putchar('\n');
  putchar('\n');

  // For each 8x8 square in output, calculate a shade and print that
  for (size_t r = 0; r < HM01B0_PIXEL_Y_NUM / 8; r++) {
    for (size_t c = 0; c < HM01B0_PIXEL_X_NUM / 8; c++) {
      uint16_t sum = 0;
      for (size_t i = 0; i < 8; i++) {
        for (size_t j = 0; j < 8; j++) {
          sum += (uint16_t)get_pixel(2 + c * 8 + j, 2 + r * 8 + i);
        }
      }
      int s = (sum + (64 * 16 / 2)) / (64 * 16);
      putchar(shades[16 - s]);
    }
    putchar('\n');
  }
}

// Outputs data as 244 lines of 324 base64 encoded bytes
static const char* b64_table =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static inline char b64_lookup(uint8_t n) {
  return b64_table[0x3f & n];
}
#define b64_line_len (HM01B0_PIXEL_X_NUM / 3 * 4 + 1)
static char b64_buf[b64_line_len + 1];

void raw() {
  puts("+++ base64 +++");

  b64_buf[b64_line_len] = 0;
  for (size_t y = 0; y < HM01B0_PIXEL_Y_NUM; y++) {
    size_t i = 0;
    for (size_t x = 0; x < HM01B0_PIXEL_X_NUM; x += 3) {
      uint8_t p0 = get_pixel(x + 0, y);
      uint8_t p1 = get_pixel(x + 1, y);
      uint8_t p2 = get_pixel(x + 2, y);
      b64_buf[i++] = b64_lookup(p0 >> 2);
      b64_buf[i++] = b64_lookup((p0 << 6) | (0xf & (p1 >> 4)));
      b64_buf[i++] = b64_lookup((p1 << 4) | (0x3 & (p2 >> 6)));
      b64_buf[i++] = b64_lookup(p2);
    }
    puts(b64_buf);
  }
  puts("--- base64 ---");
}

void raw_3() {
  for (int i = 0; i < 3; i++) {
    capture();
    raw();
    leds_out_write(~i);
  }
}

void run_classify() {
  int8_t person, not_person;
  puts("Running classification.");
  tflite_classify(&person, &not_person);
  printf("Person: %d, Not person %d\n", person, not_person);
}

void classify_demo() {
  tflite_load_person();
  puts("Loaded image with person");
  run_classify();
  tflite_load_no_person();
  puts("Loaded image without person");
  run_classify();
}

int main(void) {
  irq_setmask(0);
  irq_setie(1);
  uart_init();

  printf("Hello, %s!\n", "World");

  hm01b0_status_e status;

  // init camera
  status = hm01b0_mclk_enable(&hm01b0_cfg);
  CAM_CHECK(status, "mclk");
  status = hm01b0_init_if(&hm01b0_cfg);
  CAM_CHECK(status, "init");

  // Auto exposure struct
  memset((void*)&aecfg, 0x00, sizeof(hm01b0_ae_cfg_t));

  // Check model ID
  uint16_t modelid;
  status = hm01b0_get_modelid(&hm01b0_cfg, &modelid);
  CAM_CHECK(status, "get_modelid");
  if (!status)
    printf("model id = %04x\n", modelid);

  // Load up init script
  puts("hm01b10_init_system()");
  hm01b0_init_system(&hm01b0_cfg, (hm_script_t*)sHM01B0InitScript,
                     sizeof(sHM01B0InitScript) / sizeof(hm_script_t));

  // Initialize TFLite
  tflite_init();

  while (1) {
    // Check model ID
    status = hm01b0_get_modelid(&hm01b0_cfg, &modelid);
    CAM_CHECK(status, "get_modelid");
    printf("model id = %04x\n", modelid);

    // Mirror the bytes back
    puts("\n\nMenu");
    puts("----");
    puts("1. Calibrate Auto Exposure");
    puts("2. Capture & Show");
    puts("3. TfLM demo");
    puts("4. Capture and classify");
    printf("9. Toggle Fudge (now %d)\n", x_fudge);
    puts("0. Raw data");
    puts("-. Capture three raw frames");
    puts("> ");

    int c = readchar();
    putchar(c);
    putchar('\n');

    switch (c) {
      case '1':
        calibrate();
        break;
      case '2':
        capture();
        dump();
        break;
      case '3':
        classify_demo();
        break;
      case '4':
        capture();
        dump();
        run_classify();
        break;
      case '9':
        x_fudge = 100 - x_fudge;
        break;
      case '0':
        raw();
        break;
      case '-':
        raw_3();
        break;
      default:
        puts("???\n");
    }
  }
}
