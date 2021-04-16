// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "fake_dev.h"

/*
 * Read from registers.
 */
bool FakeDev::read(uint8_t cmd, std::vector<uint8_t> &data) {
    // Clear the whole buffer.
    std::fill(std::begin(data), std::end(data), 0);
    if ((cmd & 0x80) != 0) {
        // Register read.
        int reg = cmd & 0x7F;
        uint16_t value;
        {
            std::lock_guard<std::mutex> l(this->lock_);
            value = this->regs[reg];
        }
        // Store the value of the register into the buffer.
        if (data.size() > 0) {
            data[0] = value >> 8;
            if (data.size() > 1) {
                data[1] = value;
            }
        }
    }
    return true;
}

/*
 * Write to registers or memory.
 */
bool FakeDev::write(uint8_t cmd, const std::vector<uint8_t> &data) {
    if ((cmd & 0x80) != 0) {
        if (data.size() != 0) {
            // Register write.
            int reg = cmd & 0x7F;
            uint16_t value = data[0] << 8;
            if (data.size() > 1) {
                value |= data[1];
            }
            switch (reg) {
            default:
                break;
            case HpsReg::kSysCmd:
                // TODO: Process cmd.
                break;
            }
        }
    } else {
        if ((cmd & 0xC0) == 0) {
            // Memory write.
        } else {
            return false;
        }
    }
    return true;
}
