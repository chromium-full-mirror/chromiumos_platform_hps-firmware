// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * I2C device handler.
 */
#ifndef __I2C_H__
#define __I2C_H__

#include <stdint.h>
#include <string>

#include "dev.h"

struct i2c_msg;

namespace hps {

class I2CDev : public DevInterface {
  public:
    I2CDev(int bus, int address);
    int Open();
    bool read(uint8_t cmd, std::vector<uint8_t> &data) override;
    bool write(uint8_t cmd, const std::vector<uint8_t> &data) override;
  private:
    bool ioc(struct i2c_msg *msg, uint count);
    int bus;
    int address;
    int fd;
    std::string name;
};

}  // namespace hps

#endif // __I2C_H__
