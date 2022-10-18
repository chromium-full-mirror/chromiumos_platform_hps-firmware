// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::histogram::Histogram;
use crate::Error;
use crate::Microseconds;
use crate::Result;
use crate::Timer;
use core::ops::Range;

pub const IMAGE_WIDTH: u32 = 320;
pub const IMAGE_HEIGHT: u32 = 240;
pub const NUM_PIXELS: usize = IMAGE_WIDTH as usize * IMAGE_HEIGHT as usize;

/// The width and height of the camera's output in pixels.
pub const CAMERA_WIDTH_HEIGHT: usize = 324;

pub const CAMERA_NUM_PIXELS: usize = CAMERA_WIDTH_HEIGHT * CAMERA_WIDTH_HEIGHT;

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

pub struct ImageDataReceiver<C, T> {
    camera: C,
    timer: T,
    pub(crate) histogram: Histogram,
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

impl<C: CameraDataInterface, T: Timer> ImageDataReceiver<C, T> {
    pub(crate) fn new(camera: C, timer: T) -> Self {
        Self {
            camera,
            timer,
            histogram: Histogram::default(),
        }
    }

    /// Prepare to receive a frame.
    pub(crate) fn start(&mut self) {
        self.camera.start();
    }

    /// Returns whether we've missed the start of the frame. Result is only
    /// valid if called after start and before receive_image_data.
    pub(crate) fn has_missed_start_of_frame(&self) -> bool {
        // If there's data ready, then the frame has already started, even if we
        // haven't technically dropped any yet - the window of time when we have
        // received the first word, but haven't yet dropped it is very small.
        self.camera.is_data_ready()
    }

    /// Waits for the start of a frame, then receives image data into `buffer`.
    /// The camera must already be streaming. Returns the number of microseconds
    /// we waited for the start of the image.
    pub(crate) fn receive_image_data(
        &mut self,
        buffer: &mut [i8],
        image_dimensions: &ImageDimensions,
    ) -> Result<Microseconds> {
        if image_dimensions.row_count as usize * image_dimensions.column_count as usize
            != buffer.len()
        {
            return Err(Error::Internal);
        }
        // Column count must be a multiple of 4, since we read and write a word
        // at a time.
        if image_dimensions.column_count & 3 != 0 {
            return Err(Error::Internal);
        }
        self.histogram = Default::default();
        // TODO(dml): Change the buffer to [u32] so that we don't need this.
        if (buffer.as_ptr() as usize) & 0x3 != 0 {
            return Err(Error::Internal);
        }
        let buffer_u32 = unsafe {
            core::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u32, buffer.len() / 4)
        };
        self.transfer_image_data(buffer_u32, image_dimensions)
    }

    #[inline(never)]
    fn transfer_image_data(
        &mut self,
        buffer_u32: &mut [u32],
        image_dimensions: &ImageDimensions,
    ) -> Result<Microseconds> {
        let row_out_len = (image_dimensions.column_count >> 2) as usize;
        // Number of words per row that we put into the histogram.
        const HIST_WORDS_PER_ROW: usize = IMAGE_HEIGHT as usize / 4;
        // Number of rows at the top that are excluded from the histogram.
        const NON_HIST_ROW_COUNT: usize = (CAMERA_WIDTH_HEIGHT - IMAGE_HEIGHT as usize) / 2;
        // The rows that are included in the histogram.
        const HIST_ROW_RANGE: Range<usize> =
            NON_HIST_ROW_COUNT..CAMERA_WIDTH_HEIGHT - NON_HIST_ROW_COUNT;
        // Number of words at the start of each row, within the capture area
        // that we exclude from the histogram.
        let non_hist_words_left = if image_dimensions.column_count > image_dimensions.row_count {
            // Image is wider than it is high, so we exclude the difference
            // between width and the height from the histogram.
            (IMAGE_WIDTH - IMAGE_HEIGHT) as usize / 2 / 4
        } else {
            // Image is higher than it is wide, so no pixels are excluded from
            // each row.
            0
        };
        // Convert column start from pixels to words. We discard the low two
        // bits in the process. So if column_start is 1, 2 or 3, then we'll
        // start from 0. We could have instead said that such column_starts are
        // invalid, however it's convenient to make them valid, since a
        // column_start=2 when rotated becomes a row_start=2, which is valid.
        let left_margin_words = image_dimensions.column_start >> 2;
        let mut row_index = 0;

        // Measure how long we need to wait for the start of the image.
        let start = self.timer.now();
        self.wait_for_data()?;
        let wait_us = self.timer.elapsed_us(start);

        for row_out in buffer_u32.chunks_exact_mut(row_out_len) {
            loop {
                let non_hist_len;
                let hist_len;
                if HIST_ROW_RANGE.contains(&row_index) {
                    non_hist_len = non_hist_words_left;
                    hist_len = HIST_WORDS_PER_ROW;
                } else {
                    // We're outside the histogram rows. Don't add any part of
                    // this row to the histogram.
                    non_hist_len = HIST_WORDS_PER_ROW;
                    hist_len = 0;
                }
                // Split `row_out` into three parts, any of which may be empty.
                // The left are right parts are parts of the image and are
                // captured, but are excluded from the histogram. The middle
                // part is the only part that is included in the histogram.
                let (left_out, rest) = row_out.split_at_mut(non_hist_len);
                let (middle_out, right_out) = rest.split_at_mut(hist_len);
                // We always receive the row, even if we haven't yet reached the
                // starting row. Thay way the code to read the row gets into
                // cache.
                self.receive_row(left_out, middle_out, right_out, left_margin_words)?;
                row_index += 1;
                // Check if the row we capture above is one that we want to
                // keep. If it is, fall out of the inner loop so that we can
                // advance to the next row.
                if row_index as u32 > image_dimensions.row_start {
                    break;
                }
            }
        }
        Ok(wait_us)
    }

    fn receive_row(
        &mut self,
        left_out: &mut [u32],
        middle_out: &mut [u32],
        right_out: &mut [u32],
        left_margin_words: u32,
    ) -> Result<()> {
        // Skip pixels to the left of the cropping area.
        self.skip_words(left_margin_words)?;
        // Transfer the image data - non-histogram left edge.
        for word in left_out {
            self.wait_for_data()?;
            *word = self.camera.read_word();
        }
        // Transfer the image data - update histogram.
        for word in middle_out {
            self.wait_for_data()?;
            let value = self.camera.read_word();
            *word = value;
            // We just add one of the 4 bytes in our word to the histogram.
            // Sampling 1 in 4 bytes is sufficient for our purposes.
            // Experimentally, we could sample as much as 3/4 bytes without
            // dropping pixels, however we don't really need that much sampling.
            self.histogram.add((value & 0xff) as i8);
        }
        // Transfer the image data - non-histogram right edge.
        for word in right_out {
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
        Err(Error::CameraImageTimeout)
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
    use core::ops::Deref;
    use core::ops::DerefMut;

    use super::*;

    pub(crate) struct FakeCameraDataInterface {
        offset: usize,
        pixels: Vec<i8>,
    }

    /// A buffer for storing an image. This buffer is suitable for passing to
    /// receive_image_data, which requires 4-byte alignment.
    #[repr(align(4))]
    pub(crate) struct ImageBuffer {
        pixels: [i8; NUM_PIXELS],
    }

    impl FakeCameraDataInterface {
        pub fn from_signed_bytes(pixels: Vec<i8>) -> Self {
            Self { offset: 0, pixels }
        }

        /// Expands image data by adding borders that will be cut off during
        /// image capture. Prior to calling, image data must be 320x240.
        /// `image_dimensions` should be the same as what will be used during
        /// capture.
        pub fn frame_image(&mut self, image_dimensions: &ImageDimensions) {
            assert_eq!(self.pixels.len(), NUM_PIXELS);
            let mut new_pixels = Vec::with_capacity(CAMERA_NUM_PIXELS);
            let num_pixels_top_bottom = CAMERA_WIDTH_HEIGHT * (image_dimensions.row_start as usize);
            // The number of pixels dropped on the left is rounded down to a
            // multiple of 4. This matches the what's done when we capture
            // images, since capturing requires all reads to be 4-byte aligned.
            let num_pixels_left =
                (image_dimensions.column_start - (image_dimensions.column_start % 4)) as usize;
            let num_pixels_right =
                CAMERA_WIDTH_HEIGHT - num_pixels_left - image_dimensions.column_count as usize;
            new_pixels.extend(core::iter::repeat(0).take(num_pixels_top_bottom));
            for row in self
                .pixels
                .chunks_exact(image_dimensions.column_count as usize)
            {
                new_pixels.extend(core::iter::repeat(0).take(num_pixels_left));
                new_pixels.extend_from_slice(row);
                new_pixels.extend(core::iter::repeat(0).take(num_pixels_right));
            }
            new_pixels.extend(core::iter::repeat(0).take(num_pixels_top_bottom));
            self.pixels = new_pixels;
            assert_eq!(self.pixels.len(), CAMERA_NUM_PIXELS);
        }
    }

    impl CameraDataInterface for FakeCameraDataInterface {
        fn start(&mut self) {
            self.offset = 0;
        }

        fn signal_wait_row(&mut self) {
            self.offset = self.offset - (self.offset % CAMERA_WIDTH_HEIGHT) + CAMERA_WIDTH_HEIGHT;
            if self.offset >= self.pixels.len() {
                self.offset = 0;
            }
        }

        fn is_data_ready(&self) -> bool {
            true
        }

        fn read_word(&mut self) -> u32 {
            let v = &self.pixels[self.offset..];
            let value = u32::from_le_bytes([v[0] as u8, v[1] as u8, v[2] as u8, v[3] as u8]);
            self.offset = (self.offset + 4) % self.pixels.len();
            value
        }
    }

    impl Default for FakeCameraDataInterface {
        fn default() -> Self {
            // We need enough different values to pass entropy calculations.
            Self::from_signed_bytes((-128..=127).collect())
        }
    }

    impl Default for ImageBuffer {
        fn default() -> Self {
            Self {
                pixels: [0i8; NUM_PIXELS],
            }
        }
    }

    impl Deref for ImageBuffer {
        type Target = [i8];

        fn deref(&self) -> &Self::Target {
            &self.pixels
        }
    }

    impl DerefMut for ImageBuffer {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.pixels
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_helpers::images;
    use crate::test_helpers::png_pixels;
    use crate::timer::testing::FakeTimer;

    use super::testing::FakeCameraDataInterface;
    use super::testing::ImageBuffer;
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

    fn histogram_from_image_capture(
        pixels: &[i8],
        image_dimensions: &ImageDimensions,
    ) -> Histogram {
        let mut camera = FakeCameraDataInterface::from_signed_bytes(pixels.to_owned());
        camera.frame_image(image_dimensions);
        let mut image_receiver = ImageDataReceiver::new(camera, FakeTimer);
        let mut buffer = ImageBuffer::default();
        image_receiver
            .receive_image_data(&mut buffer, image_dimensions)
            .unwrap();
        image_receiver.histogram
    }

    // Check that image capture computes the same histogram as if we pass the
    // image data directly to Histogram::new_cropped.
    #[test]
    fn test_histogram_consistent_nomiri() {
        let pixels = png_pixels(images::NOT_PERSON).unwrap();

        let mut image_dimensions = ImageDimensions::default();
        let via_capture = histogram_from_image_capture(&pixels, &image_dimensions);
        let reference = Histogram::new_cropped(&pixels);
        assert_eq!(via_capture.count(), reference.count());
        assert_eq!(via_capture, reference);

        image_dimensions.rotate_90();
        let via_capture = histogram_from_image_capture(&pixels, &image_dimensions);
        let reference = Histogram::new_cropped(&pixels);
        // We can't actually check that the two histograms are equal in the case
        // of rotation because Histogram::new_cropped will sample different
        // pixels. The best we can do is check that the number of pixels sampled
        // is the same.
        assert_eq!(via_capture.count(), reference.count());
    }

    #[test]
    fn test_receive_image_data_nomiri() {
        let pixels = png_pixels(images::NOT_PERSON).unwrap();
        let image_dimensions = ImageDimensions::default();
        let mut camera = FakeCameraDataInterface::from_signed_bytes(pixels.to_owned());
        camera.frame_image(&image_dimensions);
        let mut image_receiver = ImageDataReceiver::new(camera, FakeTimer);
        let mut buffer = ImageBuffer::default();
        image_receiver
            .receive_image_data(&mut buffer, &image_dimensions)
            .unwrap();
        assert_eq!(pixels, *buffer);
    }

    #[test]
    fn test_receive_image_data_rotated_nomiri() {
        let mut pixels = png_pixels(images::NOT_PERSON).unwrap();
        crate::image_rotation::rotate_240_320_clockwise(&mut pixels);
        let mut image_dimensions = ImageDimensions::default();
        image_dimensions.rotate_90();
        let mut camera = FakeCameraDataInterface::from_signed_bytes(pixels.to_owned());
        camera.frame_image(&image_dimensions);
        let mut image_receiver = ImageDataReceiver::new(camera, FakeTimer);
        let mut buffer = ImageBuffer::default();
        image_receiver
            .receive_image_data(&mut buffer, &image_dimensions)
            .unwrap();
        assert_eq!(pixels, *buffer);
    }
}
