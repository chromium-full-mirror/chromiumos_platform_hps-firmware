// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use mcu_common::SOC_ROM_OFFSET;
use mcu_common::SPI_FLASH_SIZE;

// All our boards have an stm32g071.
#[cfg(all(feature = "standalone"))]
fn memory_x_contents() -> String {
    r#"MEMORY
{
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 64K
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}
"#
    .to_owned()
}

#[cfg(all(not(feature = "standalone")))]
fn memory_x_contents() -> String {
    format!(
        r#"MEMORY
{{
    HDR   (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    OTP    (r) : ORIGIN = 0x1FFF7000, LENGTH = 1K
    CFG   (rw) : ORIGIN = 0x1FFF7800, LENGTH = 128
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}}

SECTIONS
{{
    .image_hdr : {{
        KEEP (*(.image_hdr))
    }} > HDR
}}
"#,
        mcu_common::APPLICATION_START_ADDRESS,
        mcu_common::APPLICATION_VECTOR_TABLE_OFFSET,
        mcu_common::APPLICATION_VECTOR_TABLE_ADDRESS,
        mcu_common::STAGE1_SLOT_LENGTH
    )
}

fn build_spi_hash() {
    const MESSAGE: &'static str = "bios or bitstream file not found, use `./scripts/soc-load --build` to create it, or with a cros checkout, try: `env HPS_SPI_BIT=$HOME/chromiumos/src/platform/hps-firmware-images/firmware-bin/hps_platform.bit HPS_SPI_BIN=$HOME/chromiumos/src/platform/hps-firmware-images/firmware-bin/bios.bin cargo build` or see https://chromium.googlesource.com/chromiumos/platform/hps-firmware-images/";
    let out_dir =
        PathBuf::from(std::env::var("OUT_DIR").expect("Environment variable OUT_DIR undefined"));
    let spi_hash_path = out_dir.join("spi_hash.inc");
    let spi_hash_file = File::create(&spi_hash_path).expect("Could not create spi_hash.inc file");

    let bitstream_path = PathBuf::from(
        std::env::var("HPS_SPI_BIT")
            .unwrap_or("../../build/hps_platform/gateware/hps_platform.bit".to_string()),
    );
    let mut bitstream_file = File::open(bitstream_path).expect(MESSAGE);
    let soc_rom_path = PathBuf::from(
        std::env::var("HPS_SPI_BIN")
            .unwrap_or("../../build/hps_platform/software/bios/bios.bin".to_string()),
    );
    let mut soc_rom_file = File::open(soc_rom_path).expect(MESSAGE);

    let mut full_spi_binary = Vec::<u8>::new();
    bitstream_file
        .read_to_end(&mut full_spi_binary)
        .expect("Failed to read bitstream file");
    assert!(
        full_spi_binary.len() <= SOC_ROM_OFFSET as usize,
        "FPGA bitstream is too large"
    );
    full_spi_binary.resize(SOC_ROM_OFFSET as usize, 0xFF);
    soc_rom_file
        .read_to_end(&mut full_spi_binary)
        .expect("Failed to read bios file");
    assert!(
        full_spi_binary.len() <= SPI_FLASH_SIZE as usize,
        "FPGA bios is too large"
    );
    full_spi_binary.resize(SPI_FLASH_SIZE as usize, 0xFF);

    let mut openssl = Command::new("openssl")
        .arg("dgst")
        .arg("-sha256")
        .arg("-binary")
        .stdin(Stdio::piped())
        .stdout(spi_hash_file)
        .spawn()
        .expect("Failed to spawn openssl process");

    let mut stdin = openssl
        .stdin
        .take()
        .expect("Failed to open pipe to openssl");

    stdin
        .write_all(&full_spi_binary)
        .expect("Failed to write buffer to openssl");

    // although wait_with_output claims to close stdin
    // it doesnt if we have it. without this we deadlock.
    drop(stdin);

    let out = openssl
        .wait_with_output()
        .expect("Failed to wait for openssl");

    if !out.status.success() {
        eprintln!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::process::exit(1);
    }
    println!("cargo:rerun-if-changed=spi_hash.inc");
    println!("cargo:rustc-env=SPI_HASH={}", spi_hash_path.display());
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

    build_spi_hash();
}
