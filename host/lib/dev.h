// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Device access interface.
 */
#ifndef __DEV_H__
#define __DEV_H__

#include <stdint.h>
#include <vector>

#define REG_ACCESS      0x80
#define RETRIES         5

class DevInterface {
  public:
    /*
     * The size of the vector indicates the amount of data
     * to read.
     * Returns true on successful read, false on error.
     */
    virtual bool read(uint8_t cmd, std::vector<uint8_t> &data) = 0;
    /*
     * Write the data in the vector to the device.
     * Returns true on successful write, false on error.
     */
    virtual bool write(uint8_t cmd, std::vector<uint8_t> &data) = 0;
    /*
     * Read 1 register.
     * Returns value read, or -1 for error.
     */
    int readReg(int r) {
        std::vector<uint8_t> res(2);

        auto cmd = REG_ACCESS | (r & 0x7F);
        for (int i = 0; i < RETRIES; i++) {
            if (this->read(cmd, res)) {
                return (int(res[0]) << 8) | int(res[1]);
            }
        }
        return -1;
    }
    /*
     * Write 1 register.
     * Returns false on failure.
     */
    bool writeReg(int r, uint16_t data) {
        std::vector<uint8_t> buf(2);

        buf[0] = data >> 8;
        buf[1] = data & 0xFF;
        auto cmd = REG_ACCESS | (r & 0x7F);
        for (int i = 0; i < RETRIES; i++) {
            if (this->write(cmd, buf)) {
                return true;
            }
        }
        return false;
    }
};

#endif // __DEV_H__
