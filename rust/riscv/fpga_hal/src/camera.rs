// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use fpga_app::camera::ImageDataReceiver;
use fpga_app::camera::ImageDimensions;
use fpga_app::Error;
use fpga_app::Result;

pub struct DmaImageDataReceiver {
    camera: litex_pac::CAM_CONTROL,
}

impl ImageDataReceiver for DmaImageDataReceiver {
    fn receive_image_data(
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
        self.reset();
        self.wait_idle();
        self.start();
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
}

impl DmaImageDataReceiver {
    pub fn new(camera: litex_pac::CAM_CONTROL) -> Self {
        Self { camera }
    }

    fn reset(&mut self) {
        self.camera.reset.write(|w| w.reset().set_bit());
    }

    fn wait_idle(&mut self) {
        while self.camera.idle.read().idle().bit_is_clear() {}
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
            *word = self.camera.pixels.read().bits();
        }
        // Next read, if any, should wait for the start of the next row.
        self.camera.wait_row.write(|w| w.wait_row().set_bit());
        Ok(())
    }

    #[inline(always)]
    fn skip_words(&mut self, num_words: u32) -> Result<()> {
        for _ in 0..num_words {
            self.wait_for_data()?;
            self.camera.pixels.read().bits();
        }
        Ok(())
    }

    #[inline(always)]
    fn wait_for_data(&mut self) -> Result<()> {
        for _ in 0..10_000_000 {
            if self.camera.pixels_ready.read().pixels_ready().bit_is_set() {
                return Ok(());
            }
        }
        Err(Error::Camera)
    }

    fn start(&mut self) {
        self.camera.start_run.write(|w| w.start_run().set_bit());
    }
}
