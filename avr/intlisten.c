// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <stdbool.h>
#include <avr/io.h>

#define F_CPU 1000000UL
#define BAUD 9600

static bool hpsinterrupt_poll(void) {
  return PINB & _BV(PINB0);
}

static void led_init(void) {
  DDRC |= _BV(DDC7);
}

static void led_set(bool state) {
  if (state) {
    PORTC |= _BV(PORTC7);
  } else {
    PORTC &= ~_BV(PORTC7);
  }
}

static void uart_init(void) {
#include <util/setbaud.h>
  UBRR1H = UBRRH_VALUE;
  UBRR1L = UBRRL_VALUE;
#if USE_2X
  UCSR1A |= _BV(U2X1);
#else
  UCSR1A &= ~_BV(U2X1);
#endif
  UCSR1B = _BV(TXEN1);
}

static void uart_putchar(char c) {
  loop_until_bit_is_set(UCSR1A, UDRE1);
  UDR1 = c;
}

void main(void) {
  led_init();
  uart_init();
  bool curr = hpsinterrupt_poll();
  while (true) {
    uart_putchar(curr ? '!' : '.');
    led_set(curr);
    while (hpsinterrupt_poll() == curr);
    curr = !curr;
  }
}
