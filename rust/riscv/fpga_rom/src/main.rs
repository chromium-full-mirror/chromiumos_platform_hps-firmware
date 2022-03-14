// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

#[cfg(not(feature = "tflm_inference"))]
mod non_tflm_classifier;

use litex_pac;
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
unsafe fn create_classifier() -> non_tflm_classifier::NonTflmClassifier {
    non_tflm_classifier::NonTflmClassifier::sole_instance()
}

// Supply a dummy software implementation of __atomic_load_4 to work around a rustc bug:
// https://github.com/rust-lang/rust/issues/92897
#[no_mangle]
pub extern "C" fn __atomic_load_4(arg: *const usize, _ordering: usize) -> usize {
    unsafe { *arg }
}

#[inline(never)]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    let p = unsafe { litex_pac::Peripherals::steal() };
    let spi = fpga_hal::McuSpi::new(p.MCU_SPI);
    fpga_app::report_panic(spi);
    loop {
        unsafe { riscv::asm::wfi() };
    }
}
