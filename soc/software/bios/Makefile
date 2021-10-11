# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# LiteX assumes all software is built with make. This makefile calls our
# gn-based build, passing the required arguments to tell it where the current
# LiteX output directory is located.

TFLM_DIR   := $(abspath ../../../../third_party/tflm_gen)
TFLM_LIB   := $(TFLM_DIR)/libtensorflow-microlite.a

# Build TFLM as well as the BIOS
all:
	$(QUIET) echo "  MAKE tflm"
	$(QUIET) $(MAKE) -j -C $(TFLM_DIR) $(notdir $(TFLM_LIB))
	gn gen --args=litex_out_dir=\""$(shell cd ../..; pwd)"\" out
	ninja -C out
	rm -f bios.bin
	ln -s out/riscv-gcc/rom.bin bios.bin
