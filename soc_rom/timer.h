// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Functions for timing

#include <stdint.h>
#include <stdbool.h>
#include <generated/soc.h>

// Starts countdown timer from given value
void timer_start(uint32_t val);

// Reads countdown timer
uint32_t timer_read(void);

// Checks whether timer is at zero
bool timer_finished(void);

// Wait for the given number of ms
void timer_wait_ms(int millis);