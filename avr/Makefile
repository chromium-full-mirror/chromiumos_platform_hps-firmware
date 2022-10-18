# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

CC = avr-gcc
CFLAGS ?= -g -Os
CFLAGS += -mmcu=atmega16u4

.PHONY: all
all: intlisten.hex intlisten.bin

%.hex: %
	avr-objcopy -O ihex $< $@

%.bin: %
	avr-objcopy -O binary $< $@

intlisten: intlisten.o
	$(CC) $(CFLAGS) -o $@ $^
