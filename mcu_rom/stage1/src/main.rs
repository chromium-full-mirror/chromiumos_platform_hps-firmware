#![no_main]
#![no_std]

#![feature(alloc_error_handler)]

extern crate alloc;
use alloc_cortex_m::CortexMHeap;
use core::alloc::Layout;

use panic_abort as _;

extern crate stm32g0xx_hal as hal;
use cortex_m::{asm};
use cortex_m_rt::{entry};
use cortex_m_semihosting::*;

use git_version::git_version;

#[repr(C, packed(4))]
pub struct ImageHeader {
    magic: u32,
    version: u16,
    vector_addr: &'static u32,
    git_hash: &'static str,
    sig: [u8; 64],
}

extern "C" {
    pub static __RESET_VECTOR: u32;
}

#[link_section = ".image_hdr"]
#[no_mangle]
pub static __IMAGE_HDR: ImageHeader = ImageHeader{
    magic: 0xC03FEFE,
    version: 1,
    vector_addr: unsafe{ &__RESET_VECTOR },
    git_hash: git_version!(),
    sig: [0;64], // constructed with padded zeros for unsigned image.
};


#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

const HEAP_SIZE: usize = 2048; // in bytes


#[entry]
fn main() -> ! {
    unsafe{
        hprintln!("booting stage1 {:?}", __IMAGE_HDR.git_hash).unwrap();
    }

    // Initialize the allocator BEFORE you use it
    let start = cortex_m_rt::heap_start() as usize;
    unsafe { ALLOCATOR.init(start, HEAP_SIZE) }

    loop {}
}

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    asm::bkpt();
    loop {}
}
