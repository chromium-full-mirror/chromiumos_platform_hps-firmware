// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::ops::Deref;
use core::ops::DerefMut;

/// An owned block of mutable bytes. This is effectively a newtype wrapper for
/// `&'static mut [u8]` that makes code look a little nicer, especially when you
/// have references. e.g. `&MemBlock` looks nicer than `&&'static mut [u8]`.
pub struct MemBlock {
    data: &'static mut [u8],
    /// The size of the original allocation if it was allocated on the heap. It
    /// is critical when we reconstruct the Vec to free it, that we do so with
    /// the same capacity as the original Vec. Will be None if `data` didn't
    /// originate from the heap.
    #[cfg(any(feature = "std", test))]
    on_heap_capacity: Option<usize>,
}

impl MemBlock {
    /// It's safe to call this function, however some code somewhere will need
    /// to use unsafe in order to obtain the &'static mut that this function
    /// takes ownership of. It is the job of that code to make sure that it can
    /// only be called once, thus preventing aliasing.
    pub fn new(data: &'static mut [u8]) -> Self {
        Self {
            data,
            #[cfg(any(feature = "std", test))]
            on_heap_capacity: None,
        }
    }

    /// Constructs a new instance on the heap with the requested capacity.
    #[cfg(any(feature = "std", test))]
    pub fn with_capacity(capacity: usize) -> Self {
        let v = vec![0; capacity];
        let on_heap_capacity = Some(v.capacity());
        Self {
            data: Vec::leak(v),
            on_heap_capacity,
        }
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

#[cfg(any(feature = "std", test))]
impl Drop for MemBlock {
    fn drop(&mut self) {
        if let Some(capacity) = self.on_heap_capacity {
            // Safety: We're reconstructing a Vec using a pointer that was
            // obtained originally from a Vec. The alignment and capacity are
            // the same. The length doesn't matter, as long as it's less than
            // the capacity.
            unsafe { Vec::from_raw_parts(self.data.as_mut_ptr(), 0, capacity) };
        }
    }
}
