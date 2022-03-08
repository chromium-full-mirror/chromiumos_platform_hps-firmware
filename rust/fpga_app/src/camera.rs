// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Result;

pub const IMAGE_WIDTH: u32 = 320;
pub const IMAGE_HEIGHT: u32 = 240;
pub const NUM_PIXELS: usize = IMAGE_WIDTH as usize * IMAGE_HEIGHT as usize;

// Time between frames, measured in time to transmit a number of pixel lines.
// This value gives a frame rate of approximately 21Hz
pub const FRAME_LENGTH: u16 = 344 * 2;

pub const MAX_EXPOSURE: i32 = 0x1fff;
pub const MIN_EXPOSURE: i32 = 0;

// These constants are randomly generated to reduce the chances of them
// appearing by chance in actual image data. They are duplicated in the C code
// that sends the images.
pub const START_IMAGE_MARKER: [u8; 8] = [13, 174, 212, 250, 191, 30, 138, 125];
pub const END_IMAGE_MARKER: [u8; 8] = [227, 114, 212, 105, 101, 111, 21, 193];

pub const AE_TARGET: u8 = 60;

pub trait ImageDataReceiver {
    /// Configures camera DMA to receive into `buffer` then calls
    /// `trigger_callback` then waits for DMA to complete. If `trigger_callback`
    /// returns an error, then DMA is cancelled and the error is returned.
    fn receive_image_data(
        &mut self,
        buffer: &mut [i8],
        image_dimensions: &ImageDimensions,
        trigger_callback: impl FnOnce() -> Result<()>,
    ) -> Result<()>;
}

pub struct ImageDimensions {
    pub column_start: u32,
    pub column_count: u32,
    pub row_start: u32,
    pub row_count: u32,
}

// Calculate appropriate exposure settings from a single integer.
// Value can range from 0 (darkest) to 0x1fff (brightest)
// Value of 0x100 is a mid point - maximum integration but minimum gains
pub fn calculate_exposure(value: i32) -> hm01b0::Exposure {
    let mut val = value.clamp(MIN_EXPOSURE, MAX_EXPOSURE);

    if val < 0x100 {
        // If want darker image - no gain and modify integration
        hm01b0::Exposure {
            integration: ((FRAME_LENGTH as i32) * val / 0x100) as u16,
            analog_gain: 0,
            digital_gain: 0x100,
        }
    } else {
        // If want brighter image - max integration and add digital and analog gain
        let mut analog_gain = 0;
        while analog_gain < 0x30 && val >= 0x200 {
            analog_gain += 0x10;
            val /= 2;
        }
        hm01b0::Exposure {
            integration: FRAME_LENGTH,
            analog_gain,
            digital_gain: val as u16,
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) struct FakeImageDataReceiver;

    impl ImageDataReceiver for FakeImageDataReceiver {
        fn receive_image_data(
            &mut self,
            buffer: &mut [i8],
            _image_dimensions: &ImageDimensions,
            trigger_callback: impl FnOnce() -> Result<()>,
        ) -> Result<()> {
            // Inserts fake image data to pass entropy calculations.
            for (i, v) in buffer.iter_mut().enumerate() {
                *v = ((i & 0xff) as i16 - 128) as i8;
            }
            (trigger_callback)()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_exposure() -> Result<()> {
        // Neutral setting
        assert_eq!(
            hm01b0::Exposure {
                integration: FRAME_LENGTH,
                analog_gain: 0,
                digital_gain: 0x100,
            },
            calculate_exposure(0x100)
        );
        // 2x analog gain
        assert_eq!(
            hm01b0::Exposure {
                integration: FRAME_LENGTH,
                analog_gain: 0x10,
                digital_gain: 0x100,
            },
            calculate_exposure(0x200)
        );
        // Fractional digital gain required
        assert_eq!(
            hm01b0::Exposure {
                integration: FRAME_LENGTH,
                analog_gain: 0x20,
                digital_gain: 0x15c,
            },
            calculate_exposure(0x570)
        );
        // Maximum exposure
        assert_eq!(
            hm01b0::Exposure {
                integration: FRAME_LENGTH,
                analog_gain: 0x30,
                digital_gain: 0x3ff,
            },
            calculate_exposure(MAX_EXPOSURE)
        );
        // Half-exposure
        assert_eq!(
            hm01b0::Exposure {
                integration: FRAME_LENGTH / 2,
                analog_gain: 0x00,
                digital_gain: 0x100,
            },
            calculate_exposure(0x80)
        );
        // Minimum exposure
        assert_eq!(
            hm01b0::Exposure {
                integration: 0,
                analog_gain: 0x00,
                digital_gain: 0x100,
            },
            calculate_exposure(MIN_EXPOSURE)
        );
        Ok(())
    }
}
