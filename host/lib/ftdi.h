// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Access via FTDI device.
 */
#ifndef __FTDI_H__
#define __FTDI_H__

#include "dev.h"
#include "ftd2xx.h"

class Ftdi : public DevInterface {
  public:
    Ftdi(uint8_t addr) : address(addr << 1) { }
    bool Init();
    virtual bool read(uint8_t cmd, std::vector<uint8_t> &data);
    virtual bool write(uint8_t cmd, std::vector<uint8_t> &data);
  protected:
    void i2c_reset();
    bool i2c_read(std::vector<uint8_t> &data);
    bool ft_read(int count, std::vector<uint8_t> &input);
    bool ft_sendbyte(uint8_t data, std::vector<uint8_t> &b);
    bool ft_readbyte(uint8_t *result, bool nak);
    bool ft_get(std::vector<uint8_t> &get);
    int ft_put(std::vector<uint8_t> &output);
    uint8_t address;
    FT_HANDLE ftHandle;
};

#endif /* __FTDI_H__ */
