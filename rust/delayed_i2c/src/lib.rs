// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

use embedded_hal::blocking::delay::DelayUs;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;

/// Wraps an I2C implementation and adds a delay before all operations. This is
/// useful to ensure that i2c messages are not sent too close to one another,
/// which can violate the timing rules in the I2C spec. For standard mode
/// (100KHz), the time between STOP and START must be at least 4.7us. For fast
/// mode (400KHz) it must be at least 1.3us. For fast mode plus (1MHz) it must
/// be at least 0.5us.
pub struct DelayedI2c<I2c, Delay> {
    pub i2c: I2c,
    pub delay: Delay,
    pub duration_us: u32,
}

impl<I2cError, I2c, Delay> Read for DelayedI2c<I2c, Delay>
where
    I2c: Read<Error = I2cError>,
    Delay: DelayUs<u32>,
{
    type Error = I2cError;

    fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.delay.delay_us(self.duration_us);
        self.i2c.read(address, buffer)
    }
}

impl<I2cError, I2c, Delay> Write for DelayedI2c<I2c, Delay>
where
    I2c: Write<Error = I2cError>,
    Delay: DelayUs<u32>,
{
    type Error = I2cError;

    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.delay.delay_us(self.duration_us);
        self.i2c.write(address, bytes)
    }
}

impl<I2cError, I2c, Delay> WriteRead for DelayedI2c<I2c, Delay>
where
    I2c: WriteRead<Error = I2cError>,
    Delay: DelayUs<u32>,
{
    type Error = I2cError;

    fn write_read(
        &mut self,
        address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.delay.delay_us(self.duration_us);
        self.i2c.write_read(address, bytes, buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal_mock::delay;
    use embedded_hal_mock::i2c;

    #[test]
    fn test_read() {
        let expectations = [i2c::Transaction::read(0x42, vec![1, 2, 3])];
        let i2c = i2c::Mock::new(&expectations);
        let mut delayed_i2c = DelayedI2c {
            i2c,
            delay: delay::MockNoop::new(),
            duration_us: 1,
        };
        let mut result = [0u8; 3];
        delayed_i2c.read(0x42, &mut result).unwrap();
        assert_eq!(result, [1, 2, 3]);
        delayed_i2c.i2c.done();
    }

    #[test]
    fn test_write() {
        let expectations = [i2c::Transaction::write(0x42, vec![1, 2, 3])];
        let i2c = i2c::Mock::new(&expectations);
        let mut delayed_i2c = DelayedI2c {
            i2c,
            delay: delay::MockNoop::new(),
            duration_us: 1,
        };
        delayed_i2c.write(0x42, &[1, 2, 3]).unwrap();
        delayed_i2c.i2c.done();
    }

    #[test]
    fn test_write_read() {
        let expectations = [i2c::Transaction::write_read(
            0x42,
            vec![1, 2, 3],
            vec![4, 5, 6],
        )];
        let i2c = i2c::Mock::new(&expectations);
        let mut delayed_i2c = DelayedI2c {
            i2c,
            delay: delay::MockNoop::new(),
            duration_us: 1,
        };
        let mut result = [0u8; 3];
        delayed_i2c
            .write_read(0x42, &[1, 2, 3], &mut result)
            .unwrap();
        assert_eq!(result, [4, 5, 6]);
        delayed_i2c.i2c.done();
    }
}
