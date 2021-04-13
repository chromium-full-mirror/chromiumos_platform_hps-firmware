// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Main HPS class.
 */
#ifndef __HPS_H__
#define __HPS_H__

#include "dev.h"

class HPS {
  public:
	HPS(DevInterface &dev) :
		device(dev),
		state(State::init)
		{ }
	void Init();
	DevInterface &Device() { return this->device; }
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
