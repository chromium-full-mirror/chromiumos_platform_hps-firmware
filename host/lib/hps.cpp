// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Main HPS class.
 */

#include "hps.h"

namespace hps {

static const int kBlock = 256;

void HPS::Init() {
    this->state = State::init;
}

/*
 * Download data to the bank specified.
 * The HPa/HostS I2C Interface Memory Write is used.
 * The format of the data is:
 *    cmd byte
 *    32 bits of address in big endian format
 *    data
 */
int HPS::Download(int bank, uint32_t address, std::ifstream &source) {
    if (bank < 0 || bank >= 64) {
        return -1;
    }
    std::vector<uint8_t> buf(kBlock + sizeof(uint32_t));
    int bytes = 0;
    int rd;
    do {
        /*
         * Leave room for a 32 bit address at the start of the block
         * to be written.
         * The address is updated for each block to indicate
         * where this block is to be written.
         */
        buf.resize(kBlock +  sizeof(uint32_t));
        buf[0] = address >> 24;
        buf[1] = address >> 16;
        buf[2] = address >> 8;
        buf[3] = address;
        rd = source.readsome(reinterpret_cast<char *>(&buf[sizeof(uint32_t)]), kBlock);
        if (rd > 0) {
            buf.resize(rd + sizeof(uint32_t));
            if (!this->device.write(I2cMemWrite(bank), buf)) {
                break;
            }
            bytes += rd;
            address += rd;
        }
    } while (rd > 0);   // A read returning 0 indicates EOF.
    return bytes;
}

}  // namespace hps
