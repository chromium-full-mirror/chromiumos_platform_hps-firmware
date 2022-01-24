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
        buffer: &mut [u8],
        image_dimensions: &ImageDimensions,
        trigger_callback: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if image_dimensions.row_count as usize * image_dimensions.column_count as usize
            != buffer.len()
        {
            return Err(Error::InvalidArg);
        }
        self.reset();
        self.wait_idle();
        self.configure_cropping(image_dimensions);
        self.start();
        if let Err(error) = (trigger_callback)() {
            self.reset();
            self.wait_idle();
            return Err(error);
        }
        // TODO(dml): Change the buffer to [u32] so that we don't need this.
        if (buffer.as_ptr() as usize) & 0x3 != 0 {
            return Err(Error::InvalidArg);
        }
        let buffer_u32 = unsafe {
            core::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u32, buffer.len() / 4)
        };
        self.transfer_image_data(buffer_u32)?;

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
    fn transfer_image_data(&mut self, buffer_u32: &mut [u32]) -> Result<()> {
        for word in buffer_u32 {
            self.wait_for_data()?;
            *word = self.camera.pixels.read().bits();
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

    fn configure_cropping(&mut self, dim: &ImageDimensions) {
        let cam = &mut self.camera;
        // safety: The .bits calls are marked unsafe because svd2rust doesn't
        // know if every bit pattern is valid.
        unsafe {
            cam.column_start.write(|w| w.bits(dim.column_start));
            cam.column_count.write(|w| w.bits(dim.column_count));
            cam.row_start.write(|w| w.bits(dim.row_start));
            cam.row_count.write(|w| w.bits(dim.row_count));
        }
    }

    fn start(&mut self) {
        self.camera.start_run.write(|w| w.start_run().set_bit());
    }
}
