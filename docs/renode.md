# Renode

Renode is an open source software created by Antmicro for multinode embedded
emulation.

## Objective

Remove the dependencies on hardware for testing HPS and purely relying on
software.
A subset of the tests is hardware destructive (Flash erases) and does not scale
up to CQ requirements. [Design document](http://go/cros-hps-renode) provides
more details about integration of Renode with HPS.

Components emulated in Renode:
* MCU STM32G0

Components not emulated in Renode:
* Camera HM01B0
* SPI Flash GD25LQ128D
* FPGA LIFCL-17

## Installing Renode

Renode is part of cros_sdk and should be available *inside* the ChromiumOS chroot.
To install Renode outside of chroot, please follow [Antmicro's README](https://github.com/renode/renode).

## Running Renode

### Locally

Run the script *inside* the ChromiumOS chroot.

```
scripts/run-tests-renode
```

This scripts sets up Renode environment in the background and executes
hps-factory stage1 test suite.

### Commit Queue

HPS tests using Renode run automatically on each change.
These tests are included the in [hps-firmware ebuild](https://source.corp.google.com/chromeos_public/src/third_party/chromiumos-overlay/chromeos-base/hps-firmware/hps-firmware-9999.ebuild)
file.
