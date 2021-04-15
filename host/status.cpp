// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Read status registers.
 */

#include <iostream>
#include <iomanip>

#include "command.h"
#include "lib/hps.h"

namespace {

int status(HPS &hps, int argc, char *argv[]) {
    for (auto i = 0; i < 5; i++) {
        std::cout << "reg " << i << " = ";
        auto result = hps.Device().readReg(i);
        if (result < 0) {
            std::cout << "Error!" << std::endl;
        } else {
            std::cout << std::hex << std::setfill('0') << std::setw(4) << result << std::endl;
        }
    }
    return 0;
}

Command cmd("status", "Dump common status registers", status);

}  // namespace
