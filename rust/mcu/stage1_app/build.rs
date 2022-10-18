// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hmac_sha256::Hash;
use mcu_common::SOC_ROM_OFFSET;
use mcu_common::SPI_FLASH_SIZE;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::str;

// All our boards have an stm32g071.
#[cfg(all(feature = "standalone"))]
fn memory_x_contents() -> String {
    use mcu_common::MCU_CRASH_RECORD_SIZE;
    use mcu_common::MCU_RAM_SIZE;
    use mcu_common::MCU_RAM_START;

    format!(
        r#"MEMORY
{{
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 64K
    RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = {}
    CRASH_REC (rw) : ORIGIN = {}, LENGTH = {}
}}
PROVIDE(CRASH_RECORD = ORIGIN(CRASH_REC));
"#,
        MCU_RAM_SIZE - MCU_CRASH_RECORD_SIZE,
        MCU_RAM_START + MCU_RAM_SIZE - MCU_CRASH_RECORD_SIZE,
        MCU_CRASH_RECORD_SIZE
    )
}

#[cfg(all(not(feature = "standalone")))]
fn memory_x_contents() -> String {
    mcu_common::build_utils::stage1_linker_script()
}

fn build_spi_hash() {
    let out_dir =
        PathBuf::from(std::env::var("OUT_DIR").expect("Environment variable OUT_DIR undefined"));
    let spi_hash_path = out_dir.join("spi_hash.inc");
    if cfg!(feature = "no-hash-check") {
        write_fake_spi_hash_file(&spi_hash_path);
    } else {
        write_spi_hash_file(&spi_hash_path);
    }

    println!("cargo:rerun-if-changed=spi_hash.inc");
    println!("cargo:rustc-env=SPI_HASH={}", spi_hash_path.display());
}

fn add_segments_for_file(input_filename: &Path, address_range: Range<u32>, out: &mut File) {
    const MESSAGE: &str = "bios or bitstream file not found, use `./scripts/build-fpga-bitstream` and `./scripts/build-fpga-rom` to create them, or with a cros checkout, try: `env HPS_SPI_BIT=$HOME/chromiumos/src/platform/hps-firmware-images/firmware-bin/fpga_bitstream.bin HPS_SPI_BIN=$HOME/chromiumos/src/platform/hps-firmware-images/firmware-bin/fpga_application.bin cargo build` or see https://chromium.googlesource.com/chromiumos/platform/hps-firmware-images/";

    let mut f = File::open(input_filename).expect(MESSAGE);
    // Creates hashable binary segment.
    let mut binary = Vec::<u8>::new();
    f.read_to_end(&mut binary).expect("Failed to read file");
    // Round-up to next multiplication of PAGE_SIZE.
    let binary_size: usize = ((binary.len() + 255) / 256) * 256;
    assert!(
        binary_size <= (address_range.end - address_range.start) as usize,
        "File is too large"
    );
    let mut hasher = Hash::new();
    // Pad the binary with 0xFF as the size was rounded-up.
    binary.resize(binary_size, 0xFFu8);
    hasher.update(binary);
    let h = hasher.finalize();
    writeln!(
        out,
        "Segment {{ length: {}, kind: SegmentKind::DataWithHash([{}]) }},",
        binary_size,
        h.iter()
            .map(u8::to_string)
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    // Creates empty offset segment.
    writeln!(
        out,
        "Segment {{ length: {}, kind: SegmentKind::Empty }},",
        SOC_ROM_OFFSET as usize - binary_size
    )
    .unwrap();
}

fn write_spi_hash_file(spi_hash_path: &Path) {
    let mut segment_file =
        File::create(&spi_hash_path).expect("Could not create spi_hash.inc file");
    writeln!(&mut segment_file, "[").unwrap();
    let bitstream_path =
        PathBuf::from(std::env::var("HPS_SPI_BIT").unwrap_or_else(|_| {
            "../../../build/hps_platform/gateware/hps_platform.bit".to_string()
        }));
    add_segments_for_file(&bitstream_path, 0..SOC_ROM_OFFSET, &mut segment_file);
    let soc_rom_path = PathBuf::from(
        std::env::var("HPS_SPI_BIN")
            .unwrap_or_else(|_| "../../../build/hps_platform/fpga_rom.bin".to_string()),
    );
    add_segments_for_file(
        &soc_rom_path,
        SOC_ROM_OFFSET..SPI_FLASH_SIZE,
        &mut segment_file,
    );
    writeln!(&mut segment_file, "]").unwrap();
    segment_file.sync_all().unwrap();
}

fn write_fake_spi_hash_file(spi_hash_path: &Path) {
    let mut segment_file =
        File::create(&spi_hash_path).expect("Could not create spi_hash.inc file");
    writeln!(
        &mut segment_file,
        "[ Segment {{ length: {} , kind: SegmentKind::Empty }}, ]",
        mcu_common::SPI_FLASH_SIZE
    )
    .unwrap();
    segment_file.sync_all().unwrap();
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
