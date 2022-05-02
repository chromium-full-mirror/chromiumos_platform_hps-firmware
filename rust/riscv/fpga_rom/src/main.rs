// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

#[cfg(not(feature = "tflm_inference"))]
mod non_tflm_classifier;

use fpga_app::TEST_IMAGES_MAX_SIZE;
use fpga_app::TEST_IMAGES_OFFSET;
use riscv_rt::entry;

const SPI_FLASH_ORIGIN: usize = 0x20000000;

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
    app.set_test_image_area(test_image_area());
    app.run();
}

fn spi_test_data() -> &'static [u8] {
    const MB: usize = 1024 * 1024;
    unsafe { core::slice::from_raw_parts((SPI_FLASH_ORIGIN + 15 * MB) as *const u8, MB) }
}

fn test_image_area() -> &'static [u8] {
    unsafe {
        core::slice::from_raw_parts(
            (SPI_FLASH_ORIGIN + TEST_IMAGES_OFFSET as usize) as *const u8,
            TEST_IMAGES_MAX_SIZE as usize,
        )
    }
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

/// Supply a dummy software implementation of __atomic_load_4 to work around a rustc bug:
/// https://github.com/rust-lang/rust/issues/92897
///
/// # Safety
/// `arg` must point to valid memory and be correctly aligned.
#[no_mangle]
pub unsafe extern "C" fn __atomic_load_4(arg: *const usize, _ordering: usize) -> usize {
    *arg
}

#[inline(never)]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    let p = unsafe { litex_pac::Peripherals::steal() };
    let spi = fpga_hal::McuSpi::new(p.MCU_SPI);
    fpga_app::report_fatal_error(spi, mcu_common::Error::FpgaPanic);
    loop {
        unsafe { riscv::asm::wfi() };
    }
}

#[export_name = "ExceptionHandler"]
fn custom_exception_handler(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    let p = unsafe { litex_pac::Peripherals::steal() };
    let spi = fpga_hal::McuSpi::new(p.MCU_SPI);
    fpga_app::report_fatal_error(spi, mcu_common::Error::FpgaException);
    loop {
        unsafe { riscv::asm::wfi() };
    }
}

/// # Safety
/// We steal the peripherals and then access the SPI bus to the MCU. Must not be
/// called from within code that is using the MCU SPI bus. Caller must also make
/// sure data and size are valid.
/// This overrides _write from libnosys.
/// TODO(dml): Drop libnosys and implement required functions ourselves.
#[cfg(feature = "dev")]
#[no_mangle]
pub unsafe extern "C" fn _write(_handle: u32, data: *const u8, size: usize) -> usize {
    let p = litex_pac::Peripherals::steal();
    let spi = fpga_hal::McuSpi::new(p.MCU_SPI);
    let data = core::slice::from_raw_parts(data, size);
    fpga_app::write_stdout(spi, data);

    size
}
