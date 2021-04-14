// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Command handler.
 */

#include <cstdio>
#include <string.h>

#include "lib/hps.h"

int command(HPS &hps, int argc, char *argv[]) {
    int cmd;

    if (argc == 0) {
        printf("Missing command ('reset' or 'launch' expected)\n");
        return 1;
    }
    if (strcmp(argv[1], "reset") == 0) {
        cmd = 1;
    } else if (strcmp(argv[1], "launch") == 0) {
        cmd = 2;
    } else {
        printf("%s: Unknown command (%s)\n", argv[0], argv[1]);
        return 1;
    }

    for (auto i = 0; i < 5; i++) {
        printf("reg %d = %04x\n", i, hps.Device().readReg(i));
    }
    printf("Sending cmd 0x%04x to register 3\n", cmd);
    hps.Device().writeReg(3, cmd);
    return 0;
}
