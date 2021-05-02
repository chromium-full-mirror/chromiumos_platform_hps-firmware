// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::ops::Deref;
use core::ops::DerefMut;

/// An owned block of mutable bytes. This is effectively a newtype wrapper for
/// `&'static mut [u8]` that makes code look a little nicer, especially when you
/// have references. e.g. `&MemBlock` looks nicer than `&&'static mut [u8]`.
pub struct MemBlock {
    data: &'static mut [u8],
}

impl MemBlock {
    /// It's safe to call this function, however some code somewhere will need
    /// to use unsafe in order to obtain the &'static mut that this function
    /// takes ownership of. It is the job of that code to make sure that it can
    /// only be called once, thus preventing aliasing.
    pub fn new(data: &'static mut [u8]) -> Self {
        Self { data }
    }
}

impl Deref for MemBlock {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &*self.data
    }
}

impl DerefMut for MemBlock {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.data
    }
}
