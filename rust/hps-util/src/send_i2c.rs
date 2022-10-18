// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use embedded_hal::blocking::i2c;

pub struct SendI2c<T: i2c::WriteRead + i2c::Write> {
    inner: T,
}

impl<T: i2c::WriteRead + i2c::Write> SendI2c<T> {
    /// # Safety
    ///
    /// For calling this function to be safe, the type T needs to be safe to
    /// send between threads.
    pub unsafe fn new(inner: T) -> Self {
        Self { inner }
    }
}

unsafe impl<T: i2c::WriteRead + i2c::Write> Send for SendI2c<T> {}

impl<T: i2c::WriteRead + i2c::Write> i2c::WriteRead for SendI2c<T> {
    type Error = <T as i2c::WriteRead>::Error;

    fn write_read(
        &mut self,
        address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.inner.write_read(address, bytes, buffer)
    }
}

impl<T: i2c::WriteRead + i2c::Write> i2c::Write for SendI2c<T> {
    type Error = <T as i2c::Write>::Error;

    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.write(address, bytes)
    }
}
