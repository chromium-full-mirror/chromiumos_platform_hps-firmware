# MCU firmware and related code

This document describes the directory structure and code. For usage information,
see [../docs/mcu_build.md](../docs/mcu_build.md).

## Hardware-agnostic crates

As much code as possible is put into hardware agnostic crates. This allows tests
in these crates to be run on the host OS without real hardware needing to be
connected.

These crates are all in a workspace. See Cargo.toml in this directory.

### Crate: application

The `application` crate contains the logic of the application and tests for that
logic. Any interaction with hardware is done separately in the
`stage1_app` crate.

Primary responsibilities are:

 * Taking commands from the FPGA and producing responses, updating internal
   state as appropriate.
 * Taking requests to read/write registers from the host and producing
   responses, updating internal state as appropriate.

### Crate: debug_logger

A logger implementation for use in HPS.

### Crate: delayed_i2c

Wraps an i2c controller implementation and adds a delay to each operation. This
can be necessary to ensure that a START isn't such a short time after a STOP
that it violates the timing in the I2C spec.

### Crate: mcp2221

A library for using the MCP2221 USB. Supports i2c and GPIO.

### Crate: fake_i2c

The `fake_i2c` crate provides an implementation of the I2C bus that allows tests
to send and receive data over a fake bus.

### Crate: gateware_compress

A simple no-std compression algorithm designed specifically for the patterns we
see in Nexus gateware bitstreams.

### Crate: host_dev_common

Code shared between several host utilties. Mostly image-related code that is
shared between hps-factory and hps-mon.

### Crate: hps_interface

This crate defines an interface for talking to a HPS over I2C from the host
side. This is used by tests together with `fake_i2c` to test code in crates like
the `application` crate.

### Crate: hps-factory

A Linux binary that is intended to be run while the Chromebook with HPS module
is still in the factory. Responsible for post-install testing and initialization
of the HPS.

### Crate: hps-mon

A binary that talks to the HPS over SWD. Displays log messages received from the
MCU and FPGA.

### Crate: hps-util

A binary that can run on the host OS and communicate with the HPS for testing
and development purposes. The official host daemon is implemented in C++ in a
separate repository.

### Crate: i2c_peripheral

Defines an interface for working with I2C as a peripheral. This interface is
implemented in the `stm32g0_i2c_peripheral` crate and the `fake_i2c` crate.

The plan is to eventually upstream an interface like this one into the
`embedded_hal` crate.

### Crate: i2c_protocol

This crate implements the I2C protocol used by the HPS. It effectively takes
events from the underlying I2C implementation and converts them into higher
level events such as read/write register.

### Crate: libstage0

Parts of stage0 that are able to run on the host and thus can be tested.

### Crate: mcu_common

Contains code that can be shared between stage0, stage1/application and any
host-side code.

### Crate: mcu_common_testing

Provides facilities to help writing tests in crates that use definitions in
`mcu_common`.

### Crate: one_time_init

A stage1 binary used by hps-factory to perform one-time initialization of the
HPS.

### Crate: sign-rom

A host-side binary that hashes MCU stage1 binaries and embeds a signature into
it.

## Hardware-specific crates

These crates make use of hardware-specific features such as GPIO pins and
built-in peripherals. This means that they cannot be built for the host OS.

### Crate: stage0

The ROM portion of the bootloader. It presents a minimal I2C interface to the
host. Its responsibilities are:

 * Signature checking stage1.
 * Updating stage1 when requested by the host.
 * Launching stage1.

### Crate: stage1

Responsible for validating the SPI flash then launching the application, which
most likely will be part of the same binary.

### Crate: stm32_bootloader_client

A library for communicating with the factory bootloader on stm32
microcontrollers. Allows writing to flash/RAM.

### Crate: stage1_app

A stand-alone MCU binary that runs the application. This crate mostly just
contains code for interacting with the hardware. Any non-hardware-specific code
should ideally go in the `application` crate so that it can be tested on the
host OS.

### Crate: stm32g0_i2c_peripheral

An implementation of the trait defined in `i2c_peripheral` that uses real I2C
hardware found in the `stm32g0` series of microcontrollers.

Code in this crate will hopefully eventually be upstreamed into the
`stm32g0xx-hal` crate, although this depends on first upstreaming the trait that
it implements to the `embedded-hal` crate.
