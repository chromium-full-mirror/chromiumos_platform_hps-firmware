// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * Class to automatically register commands.
 */

#include <iostream>
#include <string.h>

#include "host/lib/hps.h"

class Command {
  public:
    Command(const char *name, const char *help, int (*func)(HPS &,int, char **))
        : name_(name), help_(help), func_(func), next_(nullptr) {
        // Add myself to the list of commands.
        this->next_ = list_;
        list_ = this;
    }
    /*
     * Match command and run.
     * Returns exit value.
     */
    static int Execute(const char *cmd, HPS &hps, int argc, char **argv) {
        for (auto el = list_; el != nullptr; el = el->next_) {
            if (strcmp(el->name_, cmd)==0) {
                return el->func_(hps, argc, argv);
            }
        }
        std::cerr << cmd << ": Unknown command" << std::endl;
        return 1;
    }
  protected:
    const char *name_;
    const char *help_;
    int (*func_)(HPS &, int, char **);
    Command *next_;
    static Command *list_;      // Global head of command list.
};
