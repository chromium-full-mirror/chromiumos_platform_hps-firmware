// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::fmt::Write as _;
use log::error;
use mcu_common::Buffer;
use mcu_common::MCU_CRASH_RECORD_SIZE;

extern "C" {
    static mut CRASH_RECORD: [u8; MCU_CRASH_RECORD_SIZE];
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Safety: We'll never return from this function, so if anything else has a
    // reference to CRASH_RECORD, that reference is effectively gone.
    let out = unsafe { &mut CRASH_RECORD };
    let mut out = Output {
        bytes: out,
        used: 0,
    };
    write!(&mut out, "{}\0", info).unwrap();
    error!("{}", info);

    cortex_m::peripheral::SCB::sys_reset();
}

/// Copies a crash report, if there is one into `buffer`, clearing the crash
/// report in the process.
pub(crate) fn copy_crash_report(buffer: &mut Buffer) {
    // Safety: This is the only access to `CRASH_RECORD` outside of crash
    // handlers (which don't return), so it shouldn't be possible for aliasing
    // to occur.
    let record = unsafe { &mut CRASH_RECORD };
    if record[0] != 0 {
        // Provided `buffer` is empty when passed to this function, this unwrap
        // will succeed because MCU_CRASH_RECORD_SIZE is less than the buffer
        // capacity. Checked by the test
        // i2c_buffer_larger_than_crash_record_size in i2c_protocol.
        buffer.push_bytes(record).unwrap();
    }
    // Clear crash report.
    record[0] = 0;
}

struct Output<'a> {
    bytes: &'a mut [u8],
    used: usize,
}

impl<'a> core::fmt::Write for Output<'a> {
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
