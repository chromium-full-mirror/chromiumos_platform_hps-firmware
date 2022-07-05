# Developing outside the ebuild

In ChromiumOS, the `hps-firmware` package builds all the firmware binaries
needed to run HPS on a Chromebook: FPGA bitstream, FPGA application, and
MCU application. The `hps-firmware-tools` package builds additional tools
which are useful for development and testing, for example the `hps-factory`
program. The `hps-firmware` package is included in the ChromiumOS base image,
whereas the `hps-firmware-tools` package is only included in test/dev images.

You can use normal ChromiumOS workflows to work on these packages
(see [the relevant section of the ChromiumOS
Developer Guide](https://chromium.googlesource.com/chromiumos/docs/+/HEAD/developer_guide.md#making-changes-to-packages-whose-source-code-is-checked-into-chromiumos-git-repositories)).
For example:

```
cros workon --board=brya start hps-firmware
FEATURES=test emerge-brya hps-firmware
```

However, sometimes it’s more convenient to build individual pieces of the HPS
firmware while you’re working on it. The `scripts/` subdirectory contains some
helper scripts for this purpose. These helper scripts must be run [inside the
ChromiumOS chroot](https://chromium.googlesource.com/chromiumos/docs/+/HEAD/developer_guide.md#create-a-chroot).

* `scripts/build-fpga-bitstream`

  Synthesises the FPGA gateware into a bitstream, suitable to be programmed
  onto the SPI flash and loaded by the FPGA.

* `scripts/build-fpga-rom`

  Builds the Rust code for the FPGA application and emits a raw image,
  suitable to be programmed onto the SPI flash and executed by the soft CPU in
  the FPGA. We sometimes call this the "FPGA ROM" image, because the soft CPU
  is configured to execute from a read-only memory.

* `scripts/build-fpga-rom-dev`

  As above, but builds the Rust code with development features enabled. These
  are extra features which are never included in release firmware, such as
  image capturing support (see [Capturing images](capturing_images.md)).

You can use the images built the above scripts as inputs for
`scripts/proto2-mon`. See [Working with proto2](proto2.md).

## Running unit tests

Rust code in HPS firmware follows the normal [Rust conventions for unit
testing](https://doc.rust-lang.org/rust-by-example/testing/unit_testing.html).
Rust unit tests are executed in the [test
phase](https://devmanual.gentoo.org/ebuild-writing/functions/src_test/index.html)
of the `hps-firmware` ebuild.

You can also run them outside the ebuild by using the `scripts/run-tests`
helper script.
