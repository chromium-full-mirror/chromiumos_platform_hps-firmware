// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Code in this file uses link_section = ".text.sys" so that we can control
// where it goes in the link order. See docs/optimizing_flash_speed.md

extern "C" {
    static _heap_start: u8;
    static _heap_end: u8;
}

static mut HEAP_NEXT: *const u8 = unsafe { &_heap_start };

/// # Safety
/// We steal the peripherals and then access the SPI bus to the MCU. Must not be
/// called from within code that is using the MCU SPI bus. Caller must also make
/// sure data and size are valid. This is only intended to be called by newlib.
#[no_mangle]
#[link_section = ".text.sys"]
pub unsafe extern "C" fn _write(_handle: u32, _data: *const u8, size: usize) -> usize {
    #[cfg(feature = "dev")]
    {
        let p = litex_pac::Peripherals::steal();
        let spi = fpga_hal::McuSpi::new(p.MCU_SPI);
        let data = core::slice::from_raw_parts(_data, size);
        fpga_app::write_stdout(spi, data);
    }

    size
}

#[no_mangle]
#[link_section = ".text.sys"]
pub extern "C" fn _read(_handle: u32, _data: *mut u8, _size: usize) -> usize {
    0
}

#[no_mangle]
#[link_section = ".text.sys"]
pub extern "C" fn _lseek(_handle: u32, _ptr: u32, _dir: i32) -> usize {
    0
}

#[no_mangle]
#[link_section = ".text.sys"]
pub extern "C" fn _close(_handle: u32) -> i32 {
    -1
}

#[no_mangle]
#[link_section = ".text.sys"]
pub extern "C" fn _fstat(_handle: u32, _stat: *const core::ffi::c_void) -> i32 {
    -1
}

#[no_mangle]
#[link_section = ".text.sys"]
pub extern "C" fn _isatty(_handle: u32) -> i32 {
    1
}

#[no_mangle]
#[link_section = ".text.sys"]
pub extern "C" fn _sbrk(inc: u32) -> usize {
    let next = unsafe { HEAP_NEXT.add(inc as usize) };
    if next > unsafe { &_heap_end } {
        panic!("Heap exhausted while allocating {} bytes", inc);
    }
    let result = unsafe { HEAP_NEXT as usize };
    unsafe { HEAP_NEXT = next };

    result
}
