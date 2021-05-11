// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use mcu_common::MemBlock;
use std::boxed::Box;

/// Size of our blocks used for testing purposes. This doesn't necessarily
/// have to be the same as what's used on the device since no code assumes
/// anything about block sizes.
const BLOCK_SIZE: usize = 64;

/// Holds a single `MemBlock` which can be taken and later returned. Provided
/// the block has been returned it is freed when we drop, otherwise it is
/// leaked.
pub struct TestingMemBlock {
    pointer: *mut u8,
    block: Option<MemBlock>,
}

impl TestingMemBlock {
    pub fn new() -> Self {
        let mut block = MemBlock::new(Box::leak(Box::new([0u8; BLOCK_SIZE])));
        Self {
            pointer: block.as_mut_ptr(),
            block: Some(block),
        }
    }

    pub fn take(&mut self) -> Option<MemBlock> {
        self.block.take()
    }

    pub fn put(&mut self, block: MemBlock) {
        self.block = Some(block)
    }
}

impl Drop for TestingMemBlock {
    fn drop(&mut self) {
        if let Some(mut block) = self.block.take() {
            if self.pointer == block.as_mut_ptr() {
                // safety: We have taken ownership of the same block of memory
                // that we allocated in `new`, so at this point it's ours to do
                // with as we please. We convert it back into a box which is
                // then dropped.
                unsafe {
                    Box::from_raw(self.pointer);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_destroy() {
        let mut testing_block = TestingMemBlock::new();
        let block = testing_block.take().unwrap();
        assert!(testing_block.take().is_none());
        testing_block.put(block);
    }
}
