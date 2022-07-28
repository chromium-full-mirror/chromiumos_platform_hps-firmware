# Running unsigned HPS firmware on a DUT

If you want to test your firmware changes on real HPS hardware, the easiest
way is to use an HPS proto2 board. See [Working with HPS proto2](proto2.md).
But sometimes it's necessary to test your changes on a full ChromeOS device,
for example to evaluate how a new model performs under realistic conditions.

Follow these steps:

1. Turn off write protect.

   You must enter
   [developer mode](https://chromium.googlesource.com/chromiumos/docs/+/HEAD/developer_mode.md)
   and turn off the system-wide hardware write protect (WP) signal.
   It is not possible to run unsigned HPS firmware unless WP is turned off.

   Recent ChromeOS devices support case-closed debugging (CCD). Use a Suzy-Q
   cable or ServoV4 to open the CCD interface and turn off WP. See the
   [Case Closed Debugging](https://chromium.googlesource.com/chromiumos/platform/ec/+/cr50_stab/docs/case_closed_debugging_cr50.md)
   documentation.

   Note that this makes the device insecure and thus it should not be used as
   a normal ChromeOS device (do not log in with a real user account).

1. Build the firmware with your changes.

   You can build the complete `hps-firmware` package, for example:

   ```
   cros workon --board=brya start hps-firmware
   emerge-brya hps-firmware
   ```

   You can also build individual images outside the ebuild.
   See [Developing outside the ebuild](outside_ebuild.md).

1. Manually copy firmware to the DUT.

   The usual `cros deploy` workflow is *not* usable in this scenario, because
   the filesystem paths do not match:

     `hps-firmware` builds firmware from source and installs to
     `/firmware/hps`, which does not exist in the ChromeOS disk image.

     `hps-firmare-images` copies pre-built signed firmware into
     `/usr/lib/firmware/hps`, which is loaded at runtime by hpsd.

   Instead, copy the firmware to the correct location on the DUT filesystem.
   For example:

   ```
   scp /build/brya/firmware/hps/fpga_application.bin \
       /build/brya/firmware/hps/fpga_bitstream.bin \
       /build/brya/firmware/hps/mcu_stage1.bin \
       /build/brya/firmware/hps/mcu_stage1.version.txt \
       $DUT:/usr/lib/firmware/hps/
   ```

   On the DUT, compress the images so that they have the expected names:

   ```
   ssh $DUT rm /usr/lib/firmware/hps/*.bin.xz
   ssh $DUT xz /usr/lib/firmware/hps/*.bin
   ```

1. Restart hpsd to load the firmware.

   On the DUT, run `restart hpsd`. Monitor `/var/log/messages` to confirm that
   hpsd successfully loaded and started your unsigned firmware.
