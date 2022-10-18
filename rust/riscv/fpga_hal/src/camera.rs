// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use fpga_app::camera::CameraDataInterface;

/// Receives image data from the camera gateware on our FPGA.
pub struct FpgaCameraDataInterface {
    camera: litex_pac::CAM_CONTROL,
}

impl CameraDataInterface for FpgaCameraDataInterface {
    #[inline(always)]
    fn start(&mut self) {
        self.camera.reset.write(|w| w.reset().set_bit());
        while self.camera.idle.read().idle().bit_is_clear() {}
        self.camera.start_run.write(|w| w.start_run().set_bit());
    }

    #[inline(always)]
    fn is_data_ready(&self) -> bool {
        self.camera.pixels_ready.read().pixels_ready().bit_is_set()
    }

    #[inline(always)]
    fn signal_wait_row(&mut self) {
        self.camera.wait_row.write(|w| w.wait_row().set_bit())
    }

    #[inline(always)]
    fn read_word(&mut self) -> u32 {
        self.camera.pixels.read().bits()
    }
}

impl FpgaCameraDataInterface {
    pub fn new(camera: litex_pac::CAM_CONTROL) -> Self {
        Self { camera }
    }
}
