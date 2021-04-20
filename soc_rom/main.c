// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
#include <console.h>
#include <generated/csr.h>
#include <hw/common.h>
#include <irq.h>
#include <stdio.h>
#include <uart.h>

void isr(void) {
  unsigned int irqs;
  irqs = irq_pending() & irq_getmask();
  if (irqs & (1 << UART_INTERRUPT)) {
    uart_isr();
  }
}

int main(void) {
  irq_setmask(0);
  irq_setie(1);
  uart_init();

  puts("Hello, World!\n");
  puts("Type some gibberish, watch the LED on ");

  int led_val = 0;
  while (1) {
    leds_out_write(led_val);
    int c = readchar();
    led_val ^= 1;
    putchar(c);
  }
}

// vim:sw=2:sts=2
