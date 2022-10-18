// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/// Wraps a mutable byte slice, allowing writing to it with core::fmt.
pub struct FormatOutput<'a> {
    bytes: &'a mut [u8],
    used: usize,
}

impl<'a> FormatOutput<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, used: 0 }
    }
}

impl<'a> core::fmt::Write for FormatOutput<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut s_bytes = s.as_bytes();
        let capacity = self.bytes.len() - self.used;
        // Truncate if over capacity.
        if s_bytes.len() > capacity {
            s_bytes = &s_bytes[..capacity];
        }
        let new_used = self.used + s_bytes.len();
        self.bytes[self.used..new_used].copy_from_slice(s_bytes);
        self.used = new_used;
        Ok(())
    }
}
