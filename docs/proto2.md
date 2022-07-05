# Working with HPS proto2

The HPS prototype board revision 2 ("proto2") is an external board with the
HPS circuit, plus additional components which are useful for development.
The board is not publicly available. Googlers can refer to
[go/hps-care](https://goto.google.com/hps-care) for more details about the
hardware.

[TOC]

## One-time setup

The proto2 board uses an FTDI FT4232H chip to provide UART and power control
to the host over USB, and an MCP2221 chip for I2C over USB. It needs some
extra packages and configuration on the host (your gLinux workstation) before
you can use it.

Run the setup script *inside* the ChromiumOS chroot:

```
scripts/setup
```

Also run the accompanying setup script *outside* the chroot. This script
grants permission for your user account to access the relevant USB devices:

```
scripts/setup-proto2-on-host
```

If your user account cannot access the FT4232H USB device, the helper scripts
may print an error like:

```
ValueError: The device has no langid (permission issue, no string descriptors supported or device error)
```

or:

```
Error: libusb_open() failed with LIBUSB_ERROR_ACCESS
Error: no device found
Error: unable to open ftdi device with vid 0403, pid 6011, description '*', serial '*' at bus location '*'
```

If your user account cannot access the MCP2221 USB device, the helper scripts
may print an error like:

```
Error: Failed to open HPS via I2C: USB error: Access denied (insufficient permissions)
```

## Running the MCU application

Run the helper script:

```
scripts/proto2-run
```

This helper script builds the MCU application with development features
enabled, then programs it onto the MCU flash and runs it under OpenOCD.
Example output:

```
** Programming Started **
** Programming Finished **
** Verify Started **
** Verified OK **
** Resetting Target **
Info : Listening on port 6666 for tcl connections
Info : Listening on port 4444 for telnet connections
```

Leave the script running OpenOCD in one terminal, and continue working in
another terminal. The proto2 helper scripts interact with OpenOCD over
port 4444.

*** note
**NOTE:** The first time you program the MCU flash (or if you have erased it),
the STM32 bootloader will not correctly reset into the application. As a
workaround, kill `scripts/proto2-run`, power cycle the board using
`scripts/proto2-power-cycle.py`, then run `scripts/proto2-run` again.
***

## Interacting with the MCU application

When the MCU application is built with development features enabled, it
accepts debugging commands using the [RTT (Real-Time
Transfer)](https://docs.rs/rtt-target/latest/rtt_target/) protocol.

Use the monitor program to interact with the MCU over RTT:

```
scripts/proto2-mon
```

Press enter to activate the console, then type a command.
The monitor prints messages it receives over RTT. Example output:

```
>> reset_mcu
MCU> INFO: Found expected flash chip
MCU> INFO: Default HM01B0 configuration applied
MCU> INFO: MCU application started. CPU is running at 60MHz

>> launch_app
Command LaunchApp running
Command LaunchApp complete
MCU> INFO: FPGA reported boot #1
FPGA> Hello from the Rust FPGA
FPGA> Classifier status: InitOk
```

You can also program the FPGA bitstream and FPGA application image via the MCU
using the `write_gateware` and `write_soc_rom` commands. The script assumes
you have already built the bitstream and application using the
`scripts/build-fpga-bitstream` and `scripts/build-fpga-rom-dev` helper
scripts.

Check `help` for a complete list of monitor commands.

## Running the MCU stage0 boot loader

On a real HPS peripheral, the stage0 boot loader is permanently programmed at
the beginning of flash, and it executes the stage1 application from an offset
in flash after verifying its signature. When working with proto2, we typically
skip the stage0 boot loader because it’s not necessary for development. The
`scripts/proto2-run` helper script mentioned above writes the application to
the beginning of flash; the stage0 boot loader is not involved.

However, if you are testing a change to the stage0 boot loader, you may want
to run it on proto2. There is a corresponding helper script for stage0:

```
scripts/proto2-stage0-run
```

There is no monitor command for stage0. It does not support RTT. You can
interact with stage0 using the `hps-util` Rust tool or the C++ `hps` tool from
platform2.

On proto2 the
[WP (Write Protect)](https://chromium.googlesource.com/chromiumos/docs/+/HEAD/write_protection.md)
line is controlled by a GPIO pin on the FT4232H. If you want to test stage0
signature verification, you can assert WP:

```
scripts/proto2-power-cycle.py --write-protect
```

By default the script deasserts WP.

## Accessing the UART

Note that although proto2 has UART connections, HPS firmware *does not* have
any code or FPGA logic for interacting with the UART. You will not see any
output on the UART. However, the proto2 UART connections are still useful with
CFU-Playground, where the code does interact with the UART.

To connect your terminal to the UART, run:

```
scripts/proto2-term
```

This script wraps the Litex terminal interface. Press Control-C twice in quick
succession to disconnect.
