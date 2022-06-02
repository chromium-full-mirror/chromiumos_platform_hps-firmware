// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::fmt::Display;

pub trait Timer {
    /// Returns the current time.
    fn now(&self) -> Instant;

    /// Returns the number of milliseconds since `start`.
    fn elapsed_ms(&self, start: Instant) -> Milliseconds;

    /// Returns the number of microseconds since `start`.
    fn elapsed_us(&self, start: Instant) -> Microseconds;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Microseconds(pub i32);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Milliseconds(pub i32);

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

        fn elapsed_ms(&self, _start: Instant) -> Milliseconds {
            Milliseconds(0)
        }

        fn elapsed_us(&self, _start: Instant) -> Microseconds {
            Microseconds(0)
        }
    }
}

impl core::ops::Sub for Milliseconds {
    type Output = Milliseconds;

    fn sub(self, rhs: Self) -> Self::Output {
        Milliseconds(self.0 - rhs.0)
    }
}

impl core::ops::Add for Milliseconds {
    type Output = Milliseconds;

    fn add(self, rhs: Self) -> Self::Output {
        Milliseconds(self.0 + rhs.0)
    }
}

impl Display for Milliseconds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)?;
        write!(f, "ms")
    }
}

impl Display for Microseconds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)?;
        write!(f, "us")
    }
}
