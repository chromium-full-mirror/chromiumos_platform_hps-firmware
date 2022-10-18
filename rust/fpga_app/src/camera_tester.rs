// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;

/// Checks whether image_data looks like a valid test pattern. We expect the
/// color bars test pattern, which has all bytes either 0x00 or 0xff. The input
/// data is assumed to have been converted to signed values by subtracting 128.
pub(crate) fn check_test_pattern(image_data: &[i8]) -> Result<(), Error> {
    let mut got_zero = false;
    let mut got_ff = false;
    for byte in image_data.iter().cloned() {
        // The camera gateware subtracts 128 from all bytes received from the
        // camera, encoding 0 as -0x80, and 0xff as 0x7f
        match byte {
            -0x80 => got_zero = true,
            0x7f => got_ff = true,
            _ => {
                // On some devices we see 0xfe and 0x01 as well, so we don't
                // treat other values as errors.
            }
        }
    }
    if got_zero && got_ff {
        Ok(())
    } else {
        Err(Error::SelfTestFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR_BARS: &[u8] = include_bytes!("../test_data/color-bars.raw");

    fn sign_convert_and_check(bytes: &[u8]) -> Result<(), Error> {
        let sign_converted_data: Vec<i8> = bytes
            .iter()
            .map(|byte| byte.wrapping_sub(128) as i8)
            .collect();
        check_test_pattern(&sign_converted_data)
    }

    #[test]
    fn test_check_test_pattern_nomiri() {
        assert!(sign_convert_and_check(COLOR_BARS).is_ok());
        assert!(sign_convert_and_check(&[0, 0, 0, 0xff, 0xff, 1, 0xfe, 0]).is_ok());
    }

    #[test]
    fn test_check_test_pattern_invalid() {
        assert!(sign_convert_and_check(&[0xff, 0xff, 0xff]).is_err());
        assert!(sign_convert_and_check(&[0, 0, 0, 0]).is_err());
        assert!(sign_convert_and_check(&[0, 0, 0, 0x0e, 0xf0, 0, 0, 0]).is_err());
    }
}
