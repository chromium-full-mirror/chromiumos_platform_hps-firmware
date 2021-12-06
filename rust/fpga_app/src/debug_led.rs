// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub trait DebugLed {
    fn turn_on(&mut self);
    fn turn_off(&mut self);
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) struct FakeLed;

    impl DebugLed for FakeLed {
        fn turn_on(&mut self) {}
        fn turn_off(&mut self) {}
    }
}
