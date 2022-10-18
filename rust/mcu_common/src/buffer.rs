// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::MemBlock;
use core::fmt::Debug;
use core::fmt::Formatter;
use core::ops::Deref;
use core::ops::DerefMut;

/// A fixed-sized buffer used for sendign and receiving data.
#[non_exhaustive]
pub struct Buffer {
    mem_block: MemBlock,
    size: usize,
}

impl Buffer {
    pub fn new(mem_block: MemBlock) -> Self {
        Self { mem_block, size: 0 }
    }

    pub fn copied_from(mut mem_block: MemBlock, data: &[u8]) -> Self {
        mem_block[..data.len()].copy_from_slice(data);
        Self {
            mem_block,
            size: data.len(),
        }
    }

    /// Appends `bytes` to the end of the buffer or returns an error if there is
    /// insufficient space.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.size + bytes.len() <= self.mem_block.len() {
            self.mem_block[self.size..self.size + bytes.len()].copy_from_slice(bytes);
            self.size += bytes.len();
            Ok(())
        } else {
            Err(Error::BufferOverrun)
        }
    }

    /// Appends `value` to the end of the buffer or returns an error if the
    /// buffer is already full.
    pub fn push(&mut self, value: u8) -> Result<(), Error> {
        self.push_bytes(&[value])
    }

    /// Appends `value` to the end of the buffer, in a big endian encoding or
    /// returns an error if there is insufficient space.
    pub fn push_u32_be(&mut self, value: u32) -> Result<(), Error> {
        self.push_bytes(&value.to_be_bytes())
    }

    /// Resize to `new_size`. Must be <= Buffer::CAPACITY. Buffer contents after
    /// a resize will be whatever was written to the buffer last time it was
    /// used.
    pub fn resize(&mut self, new_size: usize) {
        if new_size <= self.mem_block.len() {
            self.size = new_size;
        }
    }

    pub fn capacity(&self) -> usize {
        self.mem_block.len()
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Releases ownership of the underlying memory block to the caller.
    pub fn release(self) -> MemBlock {
        self.mem_block
    }
}

impl Debug for Buffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "{:?}", *self)
    }
}

impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.mem_block[..self.size]
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.mem_block[..self.size]
    }
}

impl PartialEq for Buffer {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Buffer {}

#[cfg(test)]
mod tests {
    use super::Buffer;
    use crate::MemBlock;

    #[test]
    fn test_buffer() {
        const CAPACITY: usize = 20;
        let mut mem_block = MemBlock::with_capacity(CAPACITY);
        {
            let mut buffer = Buffer::new(mem_block);

            buffer.push(10).unwrap();
            buffer.push(11).unwrap();
            assert_eq!(buffer.len(), 2);
            assert_eq!(*buffer, [10, 11]);
            mem_block = buffer.release();
        }

        let mut buffer = Buffer::new(mem_block);
        assert_eq!(*buffer, []);
        // Make sure we can fill to the capacity of our memory block.
        for _ in 0..CAPACITY {
            buffer.push(1).unwrap();
        }
        // Write beyond the capacity should fail.
        assert!(buffer.push(1).is_err());
    }
}
