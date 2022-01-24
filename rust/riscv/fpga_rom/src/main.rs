// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use litex_pac;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let p = litex_pac::Peripherals::take().unwrap();
    // safety: This is the only call to create_classifier and we don't run this
    // code more than once.
    let classifier = unsafe { create_classifier() };
    let mut app = fpga_app::App::new(
        fpga_hal::McuSpi::new(p.MCU_SPI),
        fpga_hal::DmaImageDataReceiver::new(p.CAM_CONTROL),
        classifier,
        fpga_hal::Timer::default(),
    );
    app.set_spi_test_data(spi_test_data());
    app.run();
}

fn spi_test_data() -> &'static [u8] {
    const MB: usize = 1024 * 1024;
    const SPI_FLASH_ORIGIN: usize = 0x20000000;
    unsafe { core::slice::from_raw_parts((SPI_FLASH_ORIGIN + 15 * MB) as *const u8, MB) }
}

/// # Safety
///
/// Must only be called once.
#[cfg(feature = "tflm_inference")]
unsafe fn create_classifier() -> tflm_inference::TflmClassifier {
    tflm_inference::TflmClassifier::sole_instance()
}

#[cfg(not(feature = "tflm_inference"))]
unsafe fn create_classifier() -> fpga_app::FakeClassifier {
    fpga_app::FakeClassifier::default()
}
