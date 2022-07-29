# Flash layout

HPS uses a discrete SPI flash chip, and the MCU also contains embedded flash.
Their layout is described below.

See also [related constants in `mcu_common`](../rust/mcu_common/src/lib.rs).

## MCU flash layout

The embedded MCU flash has a capacity of 128KiB.
It is mapped into MCU memory at address 0x08000000.

Offset  | Length        | Content
------- | ------------- | ----
0x00000 | 0x0a000 (40K) | Stage0 bootloader. Permanently write-protected in the factory.
0x0a000 | 0x16000 (88K) | Stage1 application.

## SPI flash layout

The SPI flash chip has a capacity of 16MiB.

Offset   | Length        | Content
-------- | ------------- | ----
0x000000 | 0x200000 (2M) | Bitstream. FPGA retrieves bitstream from 0x000000 by default.
0x200000 | 0x600000 (6M) | RISC-V program. ML models are compiled into the program as constants.
0x800000 | 0x700000 (7M) | Images for testing model execution in dev firmware (only with WP off).
0xf00000 | 0x100000 (1M) | Test pattern. Written and checked during the factory process.
