// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub trait Timer {
    /// Returns the current time.
    fn now(&self) -> Instant;

    /// Returns the number of milliseconds since `start`.
    fn elapsed_ms(&self, start: Instant) -> u32;
}

/// An instant in time. Representation is implementation defined, but is
/// generally CPU cycles.
#[derive(Clone, Copy, Debug)]
pub struct Instant(pub u64);

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) struct FakeTimer;

    impl Timer for FakeTimer {
        fn now(&self) -> Instant {
            Instant(0)
        }

        fn elapsed_ms(&self, _start: Instant) -> u32 {
            0
        }
    }
}
