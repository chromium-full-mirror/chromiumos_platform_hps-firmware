// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Read status registers.
 */

#include <cstdio>
#include "lib/hps.h"

int status(HPS &hps, int argc, char *argv[]) {
    for (auto i = 0; i < 5; i++) {
        printf("reg %d = ", i);
        auto result = hps.Device().readReg(i);
        if (result < 0) {
            printf("Error!\n");
        } else {
            printf("%04x\n", result);
        }
    }
    return 0;
}
