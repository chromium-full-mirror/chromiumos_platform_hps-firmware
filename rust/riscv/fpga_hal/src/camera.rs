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
        self.enable_signed_dma();
        self.set_dma_base_address(buffer.as_mut_ptr());
        self.start();
        if let Err(error) = (trigger_callback)() {
            // Make sure that no DMA is going to occur after we return.
            self.reset();
            self.wait_idle();
            return Err(error);
        }
        self.dma_wait();

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

    fn enable_signed_dma(&mut self) {
        self.camera.signed_dma.write(|w| w.signed_dma().set_bit());
    }

    fn set_dma_base_address(&mut self, address: *mut u8) {
        self.camera
            .base_address
            .write(|w| unsafe { w.bits(address as u32) });
    }

    fn start(&mut self) {
        self.camera.start_run.write(|w| w.start_run().set_bit());
    }

    fn dma_wait(&mut self) {
        while self.camera.running.read().running().bit_is_set() {}
    }
}
