/*
 * Copyright 2021 The Chromium OS Authors. All rights reserved.
 * Use of this source code is governed by a BSD-style license that can be found
 * in the LICENSE file.
 */

INCLUDE generated/output_format.ld
INCLUDE generated/regions.ld

/* The CPU reset address is defined in hps_soc.py to be the beginning of the
 * rom region, so our program entrypoint must match that. */
ENTRY(_start)
ASSERT(_start == ORIGIN(rom), "_start must be at the beginning of rom")

SECTIONS
{
  .text :
  {
    /* Place crt0.S first so that _start is at the beginning of rom. */
    *crt0.o(.text .text.*)
    /* Next place our application code: */
    riscv-gcc/obj/soc_rom/rom.bin.*.o(.text .text.*)
    /* Then place the rest of the code at a fixed offset. We expect this code
     * to change less frequently, and by placing it at a fixed offset we can
     * reduce churn when writing to SPI flash during development. */
    . = ORIGIN(rom) + 0x00040000;
    *(.text .text.*)
  } > rom

  .rodata :
  {
    . = ALIGN(8);
    *(.rodata .rodata.*)
    *(.srodata .srodata.*)
  } > rom

  .data :
  {
    . = ALIGN(8);
    _fdata = .;
    *(.data .data.*)
    *(.sdata .sdata.*)
    _edata = .;
  } > sram AT > rom

  .bss :
  {
    . = ALIGN(8);
    _fbss = .;
    *(.bss .bss.*)
    *(.sbss .sbss.*)
    _ebss = .;
  } > sram
}

/* crt0.S relies on these symbols: */
PROVIDE(_fstack = ORIGIN(sram) + LENGTH(sram) - 8);
PROVIDE(_fdata_rom = LOADADDR(.data));
PROVIDE(_edata_rom = LOADADDR(.data) + SIZEOF(.data));
