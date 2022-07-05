# Capturing images

Release builds of HPS firmware are incapable of capturing or transferring
image data by design.

However it can be useful for development and testing purposes to "see what HPS
sees". The `image-transfer` Cargo feature (disabled by default, and disallowed
in signed firmware) adds some extra code to the FPGA application which
captures and transfers image data over I2C to the host.

## Capturing images from proto2

If you are using HPS proto2 (see [Working with proto2](proto2.md)),
the monitor program can receive and save images captured by the FPGA.

As a convenience, you can use the `scripts/fpga-rom-run` wrapper script, which
first takes care of building the FPGA application with development features
enabled, and then tells the monitor to update SPI flash and launch the FPGA
application.

Pass the `--image-out` option to specify an absolute directory where images
should be saved. You can also use the `--cmd` option to enable image transfer
immediately after the FPGA application is launched:

```
scripts/fpga-rom-run --image-out $(pwd)/images/ --cmd 'transfer 1'
```

Example output:

```
     Running `target/debug/hps-mon --openocd-port 4444 --gateware /mnt/host/source/src/platform/hps-firmware2/build/hps_platform/gateware/hps_platform.bit --soc-rom /mnt/host/source/src/platform/hps-firmware2/build/hps_platform/fpga_rom.bin --test-data-dir /mnt/host/source/src/platform/hps-firmware2/test_data --cmd 'reset_mcu; write_gateware; write_soc_rom; launch_app_without_i2c' --image-out /home/dcallagh/chromiumos/src/platform/hps-firmware2/images/ --cmd 'transfer 1'`
Welcome to hps-mon. Press enter to activate console.
CRC checked 7 of 7 blocks
All blocks already up-to-date. Nothing to write.
MCU> INFO: Found expected flash chip
MCU> INFO: Default HM01B0 configuration applied
MCU> INFO: MCU application started. CPU is running at 60MHz
CRC checked 10 of 14 blocks
CRC checked 14 of 14 blocks
All blocks already up-to-date. Nothing to write.
MCU> INFO: FPGA reported boot #1
FPGA> Hello from the Rust FPGA
FPGA> Classifier status: InitOk
FPGA> Image transfer: true
Wrote "/home/dcallagh/chromiumos/src/platform/hps-firmware2/images/00000000.raw"
Wrote "/home/dcallagh/chromiumos/src/platform/hps-firmware2/images/00000000.png"
Wrote "/home/dcallagh/chromiumos/src/platform/hps-firmware2/images/00000001.raw"
Wrote "/home/dcallagh/chromiumos/src/platform/hps-firmware2/images/00000001.png"
Wrote "/home/dcallagh/chromiumos/src/platform/hps-firmware2/images/00000002.raw"
Wrote "/home/dcallagh/chromiumos/src/platform/hps-firmware2/images/00000002.png"
```

## Capturing images from a ChromeOS DUT

The `hps-factory capture-images` command can capture images. This requires
that WP is deasserted. Signed release firmware is incapable of capturing
images, and the unsigned development firmware embedded in `hps-factory` will
fail to start on a ChromeOS DUT with WP asserted.

*** note
This does not work with `hps-factory` in the test image
([yet](https://chromium-review.googlesource.com/c/chromiumos/platform/hps-firmware/+/3644931)).
You must build `hps-factory` by hand.
***
