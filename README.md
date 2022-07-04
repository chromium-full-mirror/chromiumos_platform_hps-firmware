# ChromiumOS Human Presence Sensor

The ChromiumOS Human Presence Sensor is a hardware peripheral which can
detect the presence of one or more humans in front of the Chromebook.

This repository contains source code for the firmware which runs on the
peripheral.

For more information, refer to the
[design document](https://goto.google.com/cros-hps-dd) (Googlers only).

## Documentation

* Reference material
    * [I2C protocol](docs/host_device_i2c_protocol.md)
* How-to guides for developers
    * [Building and programming the FPGA](docs/fpga_build.md)
    * [Building and programming the MCU](docs/mcu_build.md)
    * [Using an IDE](docs/ide.md)
    * [Optimizing FPGA ROM flash layout](docs/optimizing_flash_speed.md)
    * [Regenerating TfLM](docs/regen_tflm.md)
