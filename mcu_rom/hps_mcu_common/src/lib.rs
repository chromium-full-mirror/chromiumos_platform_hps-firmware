#![no_std]

#[repr(C, packed(4))]
pub struct ImageHeader {
    pub magic: u32,
    pub version: u16,
    pub vector_addr: &'static u32,
    pub git_hash: &'static str,
    pub sig: [u8; 64],
}
