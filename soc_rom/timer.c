// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "timer.h"

#include <generated/csr.h>

// Starts countdown timer from given value
void timer_start(uint32_t val) {
  timer0_en_write(0);
  timer0_reload_write(0);
  timer0_load_write(val);
  timer0_en_write(1);
}

// Reads countdown timer
uint32_t timer_read(void) {
  timer0_update_value_write(1);
  return timer0_value_read();
}

// Checks whether timer is at zero
bool timer_finished(void) {
  return timer_read() == 0;
}

// Wait for the given number of ms
void timer_wait_ms(int millis) {
  timer_start(millis * (CONFIG_CLOCK_FREQUENCY / 1000));
  while (!timer_finished()) {
    /* wait */
  }
}