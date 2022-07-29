# ChromiumOS packages for HPS firmware

The code in this repository is built by the ChromiumOS packages listed below.

Also see the [userspace code in
`platform2/hps`](https://chromium.googlesource.com/chromiumos/platform2/+/HEAD/hps/),
which is packaged as `chromeos-base/hpsd` and `chromeos-base/hps-tool`.

## `chromeos-base/hps-firmware`

Ebuild:
[chromeos-base/hps-firmware-9999.ebuild](https://chromium.googlesource.com/chromiumos/overlays/chromiumos-overlay/+/HEAD/chromeos-base/hps-firmware/hps-firmware-9999.ebuild)

Compiles the three HPS firmware images: MCU firmware, FPGA bitstream, and FPGA
application. Installs the images and associated metadata to `/firmware/hps`.

The `/firmware` directory is not included in ChromiumOS disk images.
Instead, the various firmware files are archived separately alongside the disk
image and other build artefacts.

## `chromeos-base/hps-firmware-images`

Ebuild:
[chromeos-base/hps-firmware-images-9999.ebuild](https://chromium.googlesource.com/chromiumos/overlays/chromiumos-overlay/+/HEAD/chromeos-base/hps-firmware-images/hps-firmware-images-9999.ebuild)

Installs pre-compiled, signed firmware images to `/usr/lib/firmware/hps`.
They are compressed with xz during installation.

The `/usr/lib/firmware` directory *is* included in ChromiumOS disk images.
These images are the ones that ship to users and run on their Chromebooks.

For more details about how code gets from `hps-firmware` to
`hps-firmware-images`, see [Releasing new firmware](releasing.md).

## `chromeos-base/hps-firmware-tools`

Ebuild:
[chromeos-base/hps-firmware-tools-9999.ebuild](https://chromium.googlesource.com/chromiumos/overlays/chromiumos-overlay/+/HEAD/chromeos-base/hps-firmware-tools/hps-firmware-tools-9999.ebuild)

Compiles Linux userspace utilities for developers working with HPS.
Installs them to `/usr/bin`.

This package is only included in test and dev images, it's not included in the
base image (and thus is not shipped to users). Note that test-only packages
like this one are relocated to `/usr/local` and so the binaries will appear in
the final ChromiumOS image in `/usr/local/bin`, not `/usr/bin`.

## `chromeos-base/hps-sign-rom`

Ebuild:
[chromeos-base/hps-sign-rom-9999.ebuild](https://chromium.googlesource.com/chromiumos/overlays/chromiumos-overlay/+/HEAD/chromeos-base/hps-sign-rom/hps-sign-rom-9999.ebuild)

Compiles the firmware signing utility and installs it to
`/usr/bin/hps-sign-rom`.

This package is included in the SDK. It's separate from the other utilities in
`hps-firmware-tools` to minimize the space and time impact on the SDK.

The signing utility is extracted from SDK prebuilts and committed to the
signing infrastructure source tree (currently by hand as needed).
