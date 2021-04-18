// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Access via FTDI device, using open source library.
 */
#ifndef __FTDI_H__
#define __FTDI_H__

#include <libftdi1/ftdi.h>

#include "dev.h"

namespace hps {

class Ftdi : public DevInterface {
  public:
    Ftdi(uint8_t addr) : address(addr << 1) { }
    bool Init();
    void Close();
    bool read(uint8_t cmd, std::vector<uint8_t> &data) override;
    bool write(uint8_t cmd, const std::vector<uint8_t> &data) override;
  private:
    bool check(bool cond, const char *tag);
    void i2c_reset();
    bool i2c_read(std::vector<uint8_t> &data);
    bool ft_read(size_t count, std::vector<uint8_t> &input);
    bool ft_sendbyte(uint8_t data, std::vector<uint8_t> &b);
    bool ft_readbyte(uint8_t *result, bool nak);
    bool ft_get(std::vector<uint8_t> &get);
    size_t ft_put(const std::vector<uint8_t> &output);
    void dump();
    uint8_t address;
    struct ftdi_context context_;
    std::string descr_;
    std::string manuf_;
    std::string serial_;
};

}  // namespace hps

#endif /* __FTDI_H__ */
