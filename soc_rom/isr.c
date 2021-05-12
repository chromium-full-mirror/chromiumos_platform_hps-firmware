// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <generated/csr.h>
#include <uart.h>

// The interrupt service routine.
//
// The ISR linked is called from the trap_entry entry point in crt0.
void isr(void) {
  unsigned int irqs;
  irqs = irq_pending() & irq_getmask();
  if (irqs & (1 << UART_INTERRUPT)) {
    uart_isr();
  }
}