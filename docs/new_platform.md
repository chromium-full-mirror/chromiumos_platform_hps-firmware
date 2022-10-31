# Bringing up HPS v1.0 on a new board

HPS v1.0 was originally targeted to the brya board, with the intention that it
be straight-forward to deploy to other boards.

This design document for the HPS Daemon explains how the "board info" is used
by the Daemon and browser: [HPS Host Post Implementation Design
Doc](https://docs.google.com/document/d/1_kpsWixcAg6X-kfLkWWkLrNjy3KdC_-MZ_oKc7g_SxA/edit?resourcekey=0-xSBrl0sqlZ0x-HCs1KDyqg#heading=h.9pbazlmn2u4f)

In summary, these things need to be updated or added

1. Update the new board's
   [config.star](https://source.corp.google.com/chromeos_internal/src/project/brya/taeko/config.star;l=49?q=file:taeko%20file:config.star%20hps)
   so there is a bit that indicates whether the device has an HPS v1.0
   peripheral.
2. Update the new board's [coreboot device tree
   override](https://source.corp.google.com/h/chromium/chromiumos/third_party/coreboot/+/f915fdfc6179b7306089ab9ade30baf3c6755d6e:src/mainboard/google/brya/variants/taeko/overridetree.cb;l=416-430;bpv=1;bpt=0;drc=7a25260ce3d1e7439182715175616cbd228c30d6)
   to indicate the I2C bus and address in use.
3. Add an appropriate
   [hpsd.override](https://source.corp.google.com/chromeos_public/src/overlays/overlay-brya/chromeos-base/chromeos-bsp-brya/files/hpsd.override?q=hpsd.override)
   and ensure that it is [installed by the
   bsp](https://source.corp.google.com/h/chromium/chromiumos/codesearch/+/main:src/overlays/overlay-brya/chromeos-base/chromeos-bsp-brya/chromeos-bsp-brya-0.0.2-r125.ebuild;l=60-62?q=hpsd.override).
