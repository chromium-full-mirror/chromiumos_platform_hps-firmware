# Releasing new firmware

The `hps-firmware-images` package installs signed HPS firmware images into the
ChromiumOS disk image under `/usr/lib/firmware/hps`. This is the firmware
which hpsd will load and run on the user’s Chromebook. "Releasing" new
firmware therefore means updating the signed images in `hps-firmware-images`.

NOTE: since approximately OS version 15811, no new HPS firmware images are being
built or signed. Releasing new images in the future will require re-enabling
image building and signing, by reverting the changes in
https://crrev.com/i/7027432 and https://crrev.com/c/5334546 or some other method
that builds and signs the hps-firmware package automatically.

Follow these steps:

1. Choose a suitable brya-release build to take signed firmware from.
   Typically this would be the most recent successful dev-channel brya build,
   which contains HPS firmware built from the `main` branch. Use go/deneye to
   find the build number.

1. In the `src/platform/hps-firmware-images` directory, run the helper script
   to fetch the signed firmware. For example:

   ```
   script/get_signed_binary.sh R105 14983.0.0
   ```

1. The helper script creates a git commit with the updated firmware. Use
   `git commit --amend` to reword the commit as necessary: update `BUG=`
   and `TEST=` and describe the changes included in the new firmware.

1. Test the new firmware on a DUT. You can use the standard
   [`cros workon`](https://chromium.googlesource.com/chromiumos/docs/+/HEAD/developer_guide.md#Run-cros_workon-start)
   and [`cros deploy`](https://chromium.googlesource.com/chromiumos/docs/+/HEAD/cros_deploy.md)
   workflows.

1. Upload your change for review using `repo upload`.
