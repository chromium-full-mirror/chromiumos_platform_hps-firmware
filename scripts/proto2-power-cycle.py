#!/usr/bin/env python3

# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Turns the HPS off then back on again. This is useful after flashing the HPS
# MCU for the first time, or after running a factory test in order to get back
# to the system bootloader (provided the MCU flash is empty).

from pyftdi.gpio import GpioAsyncController
import time

gpio = GpioAsyncController()
# Set GPIO pin 6 (hps power enable) as an output, then make sure it's pulled
# low (power off)
gpio.configure('ftdi://ftdi:ft4232/1', direction=0x40)
gpio.write(0)
time.sleep(0.1)
# Set GPIO pin 6 back to being an input. It'll get pulled high by the pull-up
# resistor (power on).
gpio.set_direction(0x40, 0)
