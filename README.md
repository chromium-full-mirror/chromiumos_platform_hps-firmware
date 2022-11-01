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
    * [Flash layout](docs/flash_layout.md)
    * [ChromiumOS packages for HPS firmware](docs/packages.md)
* How-to guides for developers
    * [Developing outside the ebuild](docs/outside_ebuild.md)
    * [Developing outside the ChromeOS chroot](docs/cargo_outside_chroot.md)
    * [Working with HPS proto2](docs/proto2.md)
    * [Releasing new firmware](docs/releasing.md)
    * [Running unsigned firmware on a DUT](docs/unsigned_firmware_on_dut.md)
    * [Capturing images](docs/capturing_images.md)
    * [Using an IDE](docs/ide.md)
    * [Optimizing FPGA ROM flash layout](docs/optimizing_flash_speed.md)
    * [Regenerating TfLM](docs/regen_tflm.md)
    * [Updating model](docs/updating_model.md)
