// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::fmt::Debug;
use core::fmt::Formatter;
use core::ops::Deref;
use core::ops::DerefMut;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

/// A fixed-sized buffer used for receiving i2c data. Only one instance can
/// exist at a time.
#[non_exhaustive]
#[derive(Eq, PartialEq)]
pub struct Buffer {
    size: usize,
}

const BUFFER_CAPACITY: usize = 256;

static mut BUFFER_BYTES: [u8; BUFFER_CAPACITY] = [0u8; BUFFER_CAPACITY];
static AVAILABLE: AtomicBool = AtomicBool::new(true);

impl Buffer {
    /// Attempts to construct a new instance, returning None if another instance
    /// already exists.
    pub(crate) fn new() -> Option<Self> {
        if take_true(&AVAILABLE) {
            Some(Buffer { size: 0 })
        } else {
            None
        }
    }

    /// Returns whether it should be possible to call `Buffer::new()` and get
    /// back `Some`.
    pub fn is_available() -> bool {
        AVAILABLE.load(Ordering::SeqCst)
    }

    /// Appends `value` to the end of the buffer or returns an error if the
    /// buffer is already full.
    pub fn push(&mut self, value: u8) -> Result<(), ()> {
        // safety: Safe because only a single instance of `Buffer` can exist at
        // a time, so our buffer is effectively "owned" by that instance. We
        // have an exclusive (mut) reference to that buffer.
        let bytes = unsafe { &mut BUFFER_BYTES };
        if self.size < bytes.len() {
            bytes[self.size] = value;
            self.size += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn len(&self) -> usize {
        self.size
    }
}

impl Debug for Buffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "{:?}", *self)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        AVAILABLE.store(true, Ordering::SeqCst);
    }
}

impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        // safety: Safe because only a single instance of `Buffer` can exist at
        // a time, so our buffer is effectively "owned" by that instance.
        unsafe { &BUFFER_BYTES[..self.size] }
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // safety: Safe because only a single instance of `Buffer` can exist at
        // a time, so our buffer is effectively "owned" by that instance.
        unsafe { &mut BUFFER_BYTES[..self.size] }
    }
}

/// Sets `value` to false, returning the old value.
#[cfg(not(target_arch = "arm"))]
fn take_true(value: &AtomicBool) -> bool {
    value.swap(false, Ordering::SeqCst)
}

// We'd like to use cfg(target_has_atomic = "8") here instead if that ever
// becomes stable.
#[cfg(target_arch = "arm")]
fn take_true(value: &AtomicBool) -> bool {
    let mut result = false;
    cortex_m::interrupt::free(|_| {
        result = value.load(Ordering::SeqCst);
        value.store(false, Ordering::SeqCst);
    });
    result
}

#[cfg(test)]
mod tests {
    use super::Buffer;
    use super::BUFFER_CAPACITY;

    #[test]
    fn test_buffer() {
        assert!(Buffer::is_available());
        {
            let mut buffer = Buffer::new().unwrap();
            assert!(!Buffer::is_available());
            assert!(Buffer::new().is_none());
            assert!(Buffer::new().is_none());

            buffer.push(10).unwrap();
            buffer.push(11).unwrap();
            assert_eq!(buffer.len(), 2);
            assert_eq!(*buffer, [10, 11]);
        }

        assert!(Buffer::is_available());
        let mut buffer = Buffer::new().unwrap();
        assert_eq!(*buffer, []);
        // Make sure we can fill to capacity.
        for _ in 0..BUFFER_CAPACITY {
            buffer.push(1).unwrap();
        }
        // Write beyond the capacity should fail.
        assert!(buffer.push(1).is_err());
    }
}
