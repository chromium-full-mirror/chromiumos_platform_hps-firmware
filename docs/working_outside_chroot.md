# Developing outside the ChromeOS chroot

We currently need the hps-firmware directory to be part of a ChromeOS chroot.
Some tasks can only be done inside the chroot (i.e. after having run cros_sdk).
However for some tasks, it can be convient to have the option to do them outside
the chroot.

When doing so, we want the single source of truth to be the chroot. i.e. we want
to avoid having two separate versions of things that need to be kept in sync.

Provided rustup is installed, the external toolchain will call into the chroot
and use the toolchain there. For this to work, you'll need to install bwrap:

```sh
sudo apt install bubblewrap
```
