// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Main HPS class.
 */
#ifndef __HPS_H__
#define __HPS_H__

#include <iostream>
#include <fstream>

#include "dev.h"

class HPS {
  public:
    HPS(DevInterface &dev) :
        device(dev),
        state(State::init)
        { }
    void Init();
    DevInterface &Device() { return this->device; }
    /*
     * Per the HPS/Host I2C Interface, the bank
     * must be between 0-63 inclusive.
     * The length of data written is returned, or -1
     * in the case of an error.
     */
    int Download(int bank, uint32_t address, std::ifstream &source);
  protected:
    enum State {
        init,
        wait_stage0,
        wait_appl,
    };
    DevInterface &device;
    State state;
};

#endif // __HPS_H__
