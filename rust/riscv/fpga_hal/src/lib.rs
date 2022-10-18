// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

mod camera;
mod spi;
mod timer;

pub use camera::FpgaCameraDataInterface;
pub use spi::McuSpi;
pub use timer::Timer;

// Include constants defined by LiteX. These have been converted to Rust by
// bindgen in our build.rs. We ignore dead code because LiteX defines a lot of
// constants that we don't use.
#[allow(dead_code)]
mod soc {
    include!(concat!(env!("OUT_DIR"), "/soc.rs"));
}
