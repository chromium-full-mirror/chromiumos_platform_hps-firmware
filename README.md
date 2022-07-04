# ChromiumOS Human Presence Sensor

The ChromiumOS Human Presence Sensor is a hardware peripheral which can
detect the presence of one or more humans in front of the Chromebook.

This repository contains source code for the firmware which runs on the
peripheral.

For more information, refer to the
[design document](https://goto.google.com/cros-hps-dd) (Googlers only).

## Initial Setup

1.  Run the setup script. This will set up git submodules and prompt to install missing packages.
    1.  `./scripts/setup`

1.  To install Radiant you can try to run directly from CLI the extracted binary. In some cases this may fail with "Floating point exception." error.

        In that case run: `./2.2.0.97.3_Radiant_lin.run --console --prefix ~/lscc/radiant/2.2`

        Analogously for any update binaries available.

    2.  Set `RADIANT_DIR` to the path where it is installed. e.g. in your .bashrc, you  might add:
        1.  `export RADIANT_DIR=$HOME/lscc/radiant/2.2/bin/lin64`
    3.  Run `sudo ~/lscc/radiant/2.2/programmer/data/vmdata/udevsetup_ubuntu`
        (or one of the other udevsetup scripts) to correctly configure USB
        subsystem for Radiant.

## Building and programming:

* FPGA SoC: [FPGA Build and Program Guide](docs/fpga_build.md)
* [MCU](docs/mcu_build.md)

## Editing code

If your editor has IDE features for Python (e.g. vscode with the Python
extension installed), then you should open it with the appropriate environment
variables set. e.g.:

```sh
(source environment; code .)
```
