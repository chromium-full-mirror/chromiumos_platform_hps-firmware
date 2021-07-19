// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "hm01b0_litex.h"

#include <hps_i2c.h>

#include "camera.h"
#include "hm01b0_platform.h"

// We include hm01b0.c rather than compiling it separately so that we can
// override its hm01b0_platform.h. One alternative to doing this, would be to
// symlink their hm01b0.c here, but then we'd need a way to skip auto-formatting
// their file and there might be a risk of accidentally editing it and making it
// not be a symlink, at which point we've got a differently licensed file
// outside of third_party.
#include <SparkFun_HM01B0_Camera_ArduinoLibrary/src/hm01b0.c>

// So many unused parameters
#define UNUSED(x) ((void)(x))

static hm01b0_status_e init(hm01b0_cfg_t* psCfg, void* arg) {
  UNUSED(psCfg);
  UNUSED(arg);
  // Nothing to do. We don't control power to the camera.
  return HM01B0_ERR_OK;
}

static hm01b0_status_e write(hm01b0_cfg_t* psCfg,
                             uint16_t ui16Reg,
                             uint8_t* pui8Value,
                             uint32_t ui32NumBytes,
                             void* arg) {
  UNUSED(psCfg);
  UNUSED(arg);
  UNUSED(ui32NumBytes);
  // TODO(jakubm): Extend the API to support numBytes greater than one.
  int ret = write_camera_i2c(&ui16Reg, 1, pui8Value);
  return ret ? HM01B0_ERR_I2C : HM01B0_ERR_OK;
}

static hm01b0_status_e read(hm01b0_cfg_t* psCfg,
                            uint16_t ui16Reg,
                            uint8_t* pui8Value,
                            uint32_t ui32NumBytes,
                            void* arg) {
  UNUSED(psCfg);
  UNUSED(arg);
  UNUSED(ui32NumBytes);
  // TODO(jakubm): Extend the API to support numBytes greater than one.
  int ret = read_camera_i2c(&ui16Reg, 1, pui8Value);
  return ret ? HM01B0_ERR_I2C : HM01B0_ERR_OK;
}

static hm01b0_status_e mclk(hm01b0_cfg_t* psCfg, bool enable, void* arg) {
  UNUSED(psCfg);
  UNUSED(arg);
  UNUSED(enable);
  // Nothing to do here, since MCLK is output by the MCU.
  return HM01B0_ERR_OK;
}

static hm01b0_status_e trig(hm01b0_cfg_t* psCfg, bool enable, void* arg) {
  UNUSED(psCfg);
  UNUSED(arg);
  uint8_t val = cam_control_w_read();
  if (enable) {
    val |= (1 << CSR_CAM_CONTROL_W_TRIG_OFFSET);
  } else {
    val &= ~(1 << CSR_CAM_CONTROL_W_TRIG_OFFSET);
  }
  cam_control_w_write(val);
  return HM01B0_ERR_OK;
}

static hm01b0_status_e deinit(hm01b0_cfg_t* psCfg, void* arg) {
  UNUSED(psCfg);
  UNUSED(arg);
  // Nothing to do. We don't control power to the camera.
  return HM01B0_ERR_OK;
}

// Defines the interface
hm01b0_if_t hm01b0_litex_if = {
    .init = &init,
    .write = &write,
    .read = &read,
    .mclk = &mclk,
    .trig = &trig,
    .deinit = &deinit,
    .arg = NULL,
};

hm01b0_cfg_t hm01b0_cfg = {
    .interface = &hm01b0_litex_if,
};
