fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
  # The header doesn't need 256 bytes, however the vector table is located at
  # the start of FLASH and the vector table must be 256 byte aligned, so we
  # give the extra space to the header.
  HDR   (rx) : ORIGIN = 0x08010000, LENGTH = 256
  FLASH (rx) : ORIGIN = 0x08010100, LENGTH = 64K - LENGTH(HDR)
  OTP    (r) : ORIGIN = 0x1FFF7000, LENGTH = 1K
  CFG   (rw) : ORIGIN = 0x1FFF7800, LENGTH = 128
  RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}

SECTIONS
{
    .image_hdr : {
        KEEP (*(.image_hdr))
    } > HDR
}
"#
}

fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let memory_x_filename = out_dir.join("memory.x");
    let contents = memory_x_contents();
    // We only write memory.x if it doesn't exist or needs changing, otherwise
    // we'll trigger a rebuild even if no files have changed.
    if std::fs::read_to_string(&memory_x_filename).unwrap_or_default() != contents {
        if let Err(error) = std::fs::write(&memory_x_filename, memory_x_contents()) {
            panic!("Failed to write {}: {}", memory_x_filename.display(), error);
        }
    }
    // Tell cargo to look for memory.x in our out dir.
    println!("cargo:rustc-link-search={}", out_dir.display());
    // Tell cargo to rebuild if memory.x has changed.
    println!("cargo:rerun-if-changed=memory.x");
}
