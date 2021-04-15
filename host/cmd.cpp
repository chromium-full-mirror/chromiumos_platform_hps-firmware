// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Command handler.
 */

#include <iostream>
#include <iomanip>
#include <string.h>

#include "command.h"
#include "lib/hps.h"

namespace {

int sendcmd(HPS &hps, int argc, char *argv[]) {
    int cmd;

    if (argc <= 1) {
        std::cerr << "Missing command ('reset' or 'launch' expected)" << std::endl;
        return 1;
    }
    if (strcmp(argv[1], "reset") == 0) {
        cmd = 1;
    } else if (strcmp(argv[1], "launch") == 0) {
        cmd = 2;
    } else {
        std::cerr << argv[0] << ": Unknown command (" << argv[1] << ")" << std::endl;
        return 1;
    }

    for (auto i = 0; i < 5; i++) {
        std::cout << "reg " << i << " = ";
        std::cout << std::hex << std::setfill('0') << std::setw(4) << hps.Device().readReg(i);
        std::cout << std::endl;
    }
    std::cout << "Sending cmd value ";
    std::cout << std::hex << std::setfill('0') << std::setw(4) << cmd;
    std::cout << " to register 3" << std::endl;
    if (hps.Device().writeReg(3, cmd)) {
        std::cout << "Success!" << std::endl;
    } else {
        std::cout << "Write failed!" << std::endl;
    }
    return 0;
}

Command cmd("cmd", "Send command (hps cmd <cmd>)", sendcmd);

}  // namespace
