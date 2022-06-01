// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Error;
use crate::Result;

pub const IMAGE_WIDTH: u32 = 320;
pub const IMAGE_HEIGHT: u32 = 240;
pub const NUM_PIXELS: usize = IMAGE_WIDTH as usize * IMAGE_HEIGHT as usize;

/// The width and height of the camera's output in pixels.
pub const CAMERA_WIDTH_HEIGHT: usize = 324;

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

pub trait CameraDataInterface {
    /// Reset the interface then signal that the next data read should be from
    /// the start of frame.
    fn start(&mut self);

    /// Signals that the next data read should be from the start of a row.
    fn signal_wait_row(&mut self);

    /// Returns whether the next word of data is ready.
    fn is_data_ready(&self) -> bool;

    /// Reads a word of image data. Calling this clears `is_data_ready`. Return
    /// value if `is_data_ready` isn't true is unspecified.
    fn read_word(&mut self) -> u32;
}

pub struct ImageDataReceiver<C> {
    camera: C,
}

pub struct ImageDimensions {
    pub column_start: u32,
    pub column_count: u32,
    pub row_start: u32,
    pub row_count: u32,
}

impl Default for ImageDimensions {
    fn default() -> Self {
        Self {
            column_start: (CAMERA_WIDTH_HEIGHT as u32 - IMAGE_WIDTH) / 2,
            column_count: IMAGE_WIDTH,
            row_start: (CAMERA_WIDTH_HEIGHT as u32 - IMAGE_HEIGHT) / 2,
            row_count: IMAGE_HEIGHT,
        }
    }
}

impl<C: CameraDataInterface> ImageDataReceiver<C> {
    pub(crate) fn new(camera: C) -> Self {
        Self { camera }
    }

    /// Waits for the start of a frame, then receives image data into `buffer`.
    /// The camera must already be streaming.
    pub(crate) fn receive_image_data(
        &mut self,
        buffer: &mut [i8],
        image_dimensions: &ImageDimensions,
    ) -> Result<()> {
        if image_dimensions.row_count as usize * image_dimensions.column_count as usize
            != buffer.len()
        {
            return Err(Error::InvalidArg);
        }
        // Column count must be a multiple of 4, since we read and write a word
        // at a time.
        if image_dimensions.column_count & 3 != 0 {
            return Err(Error::InvalidArg);
        }
        self.camera.start();
        // TODO(dml): Change the buffer to [u32] so that we don't need this.
        if (buffer.as_ptr() as usize) & 0x3 != 0 {
            return Err(Error::InvalidArg);
        }
        let buffer_u32 = unsafe {
            core::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u32, buffer.len() / 4)
        };
        self.transfer_image_data(buffer_u32, image_dimensions)?;

        Ok(())
    }

    #[inline(never)]
    fn transfer_image_data(
        &mut self,
        buffer_u32: &mut [u32],
        image_dimensions: &ImageDimensions,
    ) -> Result<()> {
        let row_out_len = (image_dimensions.column_count >> 2) as usize;
        // Convert column start from pixels to words. We discard the low two
        // bits in the process. So if column_start is 1, 2 or 3, then we'll
        // start from 0. We could have instead said that such column_starts are
        // invalid, however it's convenient to make them valid, since a
        // column_start=2 when rotated becomes a row_start=2, which is valid.
        let left_margin_words = image_dimensions.column_start >> 2;
        let mut row_index = 0;
        for row_out in buffer_u32.chunks_exact_mut(row_out_len) {
            loop {
                // We always receive the row, even if we haven't yet reached the
                // starting row. Thay way the code to read the row gets into
                // cache.
                self.receive_row(row_out, left_margin_words)?;
                row_index += 1;
                if row_index >= image_dimensions.row_start {
                    break;
                }
            }
        }
        Ok(())
    }

    fn receive_row(&mut self, row_out: &mut [u32], left_margin_words: u32) -> Result<()> {
        // Skip pixels to the left of the cropping area.
        self.skip_words(left_margin_words)?;
        // Transfer the image data.
        for word in row_out {
            self.wait_for_data()?;
            *word = self.camera.read_word();
        }
        // Next read, if any, should wait for the start of the next row.
        self.camera.signal_wait_row();
        Ok(())
    }

    #[inline(always)]
    fn skip_words(&mut self, num_words: u32) -> Result<()> {
        for _ in 0..num_words {
            self.wait_for_data()?;
            self.camera.read_word();
        }
        Ok(())
    }

    #[inline(always)]
    fn wait_for_data(&mut self) -> Result<()> {
        for _ in 0..10_000_000 {
            if self.camera.is_data_ready() {
                return Ok(());
            }
        }
        Err(Error::Camera)
    }
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

impl ImageDimensions {
    /// Rotates dimensions by 90 degrees. Clockwise or anticlockwise are not
    /// specified, since the result would be the same.
    pub(crate) fn rotate_90(&mut self) {
        core::mem::swap(&mut self.column_start, &mut self.row_start);
        core::mem::swap(&mut self.column_count, &mut self.row_count);
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    #[derive(Default)]
    pub(crate) struct FakeCameraDataInterface {
        value: u32,
    }

    impl CameraDataInterface for FakeCameraDataInterface {
        fn start(&mut self) {
            self.value = 0x04030201;
        }

        fn signal_wait_row(&mut self) {}

        fn is_data_ready(&self) -> bool {
            true
        }

        fn read_word(&mut self) -> u32 {
            // We need enough different values to pass entropy calculations.
            self.value = self.value.wrapping_add(0x04040404);
            self.value
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
