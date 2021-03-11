// Platform specific shim for LiteX SOC
// See src/platforms/README.md in the SparkFun HM01B0 library
#ifndef _HM01B0_LITEX_H_
#define _HM01B0_LITEX_H_

#include <SparkFun_HM01B0_Camera_ArduinoLibrary/src/hm01b0.h>
#include <generated/csr.h>

// Low speed interface
extern hm01b0_cfg_t hm01b0_cfg;

// High speed interface - macros

// (currently unused)
#define HM01B0_READ_VSYNC \
  (!!(cam_control_r_read() & (1 << CSR_CAM_CONTROL_R_FVLD_OFFSET)))

// Evaluate to either 0 or 1 indicating the level of the HSYNC pin
#define HM01B0_READ_HSYNC \
  (!!(cam_control_r_read() & (1 << CSR_CAM_CONTROL_R_LVLD_OFFSET)))

// Evaluate to either 0 or 1 indicating the level of the PCLK pin
#define HM01B0_READ_PCLK \
  (!!(cam_control_r_read() & (1 << CSR_CAM_CONTROL_R_PCLK_OFFSET)))

// Evaluate to an 8-bit quantity describing logic levels on data lines D0-7
#define HM01B0_READ_BYTE (cam_control_dat_read())

#endif  // _HM01B0_LITEX_H_
