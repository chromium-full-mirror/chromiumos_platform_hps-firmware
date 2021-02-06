#![no_main]
#![no_std]

use panic_abort as _;

extern crate stm32g0xx_hal as hal;
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


#[entry]
fn main() -> ! {
    unsafe{
        hprintln!("booting stage1 {:?}", __IMAGE_HDR.git_hash).unwrap();
    }
    loop {}
}
