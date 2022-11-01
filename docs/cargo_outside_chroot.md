# Developing with Cargo outside the ChromeOS chroot

Sometimes when developing Rust code (e.g when using an IDE) it can be
convenient to be able to run cargo from outside the chroot.

To facilitate this we wrap `cargo`, `rustc` and `rustfmt`. The
`rust-toolchain.toml` file in project root directory causes the tools to be
invoked via `scripts/chroot-shim` which uses `bwrap` to emulate the chroot
configuration and translate paths of the results.

By wrapping the tools we avoid having to maintain complete duplicate Rust
configurations inside and outside the chroot.

## Setting up Outside the chroot

1.  Install rustup by following the instructions at
    [rustup.rs](https://rustup.rs).
2.  Install bwrap: `sudo apt install bubblewrap`
