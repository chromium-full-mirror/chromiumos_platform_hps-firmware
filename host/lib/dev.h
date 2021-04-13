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

#define REG_ACCESS	0x80

class DevInterface {
  public:
	virtual bool read(uint8_t cmd, std::vector<uint8_t> &data) = 0;
	virtual bool write(uint8_t cmd, std::vector<uint8_t> &data) = 0;
	/*
	 * Read 1 register.
	 */
	int readReg(int r) {
		std::vector<uint8_t> res(2);

		if (!this->read(REG_ACCESS | (r & 0x7F), res)) {
			return -1;
		}
		return (int(res[0]) << 8) | int(res[1]);
	}
	/*
	 * Write 1 register.
	 */
	bool writeReg(int r, uint16_t data) {
		std::vector<uint8_t> res(2);

		res[0] = data >> 8;
		res[1] = data & 0xFF;
		return this->write(REG_ACCESS | (r & 0x7F), res);
	}
};

#endif // __DEV_H__
