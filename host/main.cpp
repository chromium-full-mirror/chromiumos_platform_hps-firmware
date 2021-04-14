// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Main command program.
 */

#include <cstdio>
#include <memory>
#include <getopt.h>
#include <stdlib.h>
#include <string.h>

#include "lib/ftdi.h"
#include "lib/hps.h"
#include "lib/i2c.h"

// TODO: Refactor to allow sub-commands to add themselves.
int status(HPS &, int, char *argv[]);
int command(HPS &, int, char *argv[]);
int download(HPS &, int, char *argv[]);

static struct {
    const char *cmd;
    int (*func)(HPS &, int, char *[]);
    const char *desc;
} commands[] = {
{    "status", status, "Dump status registers" },
{    "cmd", command, "Send command to HPS (hps cmd <value>)" },
{    "dl", download, "Download file to HPS (hps dl <bank-id> <file>)" },
};

int main(int argc, char *argv[]) {
    int c;
    int bus = 2;
    int addr = 0x30;
    bool use_ftdi = false;

    while (true) {
        int option_index = 0;
        static struct option long_options[] = {
            {"bus",        required_argument, 0,  'b' },
            {"addr",    required_argument, 0,  'a' },
            {"ftdi",    no_argument,       0,  'f' },
            {0,         0,                 0,  0 }
        };
        c = getopt_long(argc, argv, "a:b:f",
                long_options, &option_index);
        if (c == -1) {
            break;
        }
        switch (c) {
        case 'a':
            addr = atoi(optarg);
            break;

        case 'b':
            bus = atoi(optarg);
            break;

        case 'f':
            use_ftdi = true;
            break;

        default:
            return 1;
        }
    }
    if (optind >= argc) {
        printf("Missing command - valid commands are:\n");
        for (uint i = 0; i < sizeof(commands)/sizeof(commands[0]); i++) {
            printf(" %s - %s\n", commands[i].cmd, commands[i].desc);
        }
        return 1;
    }
    std::unique_ptr<DevInterface> dev;
    if (use_ftdi) {
        auto ftdi = new Ftdi(addr);
        if (!ftdi->Init()) {
            return 1;
        }
        dev.reset(ftdi);
    } else {
        auto i2c = new I2CDev(bus, addr);
        if (i2c->Open() < 0) {
            return 1;
        }
        dev.reset(i2c);
    }
    auto hps = new HPS(*dev);
    for (uint i = 0; i < sizeof(commands)/sizeof(commands[0]); i++) {
        if (strcmp(argv[optind], commands[i].cmd)==0) {
            // Pass new argc/argv to the command for any following arguments.
            // argv[0] is command name.
            int cmd_argc = argc - optind;
            char **cmd_argv = &argv[optind];
            return commands[i].func(*hps, cmd_argc, cmd_argv);
        }
    }
    printf("%s: Unknown command (%s)\n", argv[0], argv[optind]);
    return 1;
}
