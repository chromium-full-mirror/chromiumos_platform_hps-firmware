// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Fake device for HPS testing.
 */
#ifndef __FAKE_DEV_H__
#define __FAKE_DEV_H__

#include <mutex>

#include "hps_reg.h"
#include "dev.h"

class FakeDev : public DevInterface {
  public:
    FakeDev() {
        std::fill(std::begin(this->regs), std::end(this->regs), 0);
        regs[HpsReg::kMagic] = kHpsMagic;
    }
    // Device interface
    virtual bool read(uint8_t cmd, std::vector<uint8_t> &data);
    virtual bool write(uint8_t cmd, const std::vector<uint8_t> &data);
    // Control methods for fake.
    void SetReg(int reg, uint16_t value) {
        if (reg < 0 || reg > HpsReg::kMax) {
            return;
        }
        std::lock_guard<std::mutex> l(this->lock_);
        this->regs[reg] = value;
    }
  protected:
    std::mutex lock_;
    uint16_t regs[HpsReg::kNumRegs];
};

#endif // __FAKE_DEV_H__
