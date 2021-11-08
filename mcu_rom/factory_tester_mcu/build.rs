// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::ffi::OsString;
use std::process::Command;

fn main() {
    write_memory_x();
    write_compressed_gateware();
}

// In our production config, we completely avoid using the MCU's flash. Our
// "flash" is actually RAM. We just call it "FLASH" for compatibility with the
// upstream-provided linker script. The factory bootloader uses the first 0x2700
// bytes of RAM, so our "FLASH" cannot overlap with this space. Our RAM can
// overlap wit the space, since we don't write it until after we've started, at
// which point the factory bootloader doesn't need whatever it wrote there.
#[cfg(not(any(feature = "dev", feature = "stage1")))]
fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
    FLASH (rx) : ORIGIN = 0x20002700, LENGTH = 26880
    RAM  (rw) : ORIGIN = 0x20000400, LENGTH = 8K
}
"#
}

#[cfg(feature = "stage1")]
fn memory_x_contents() -> String {
    #[cfg(feature = "dev")]
    compile_error!("dev and stage1 features are currently incompatible");

    format!(
        r#"MEMORY
{{
    HDR   (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
    FLASH (rx) : ORIGIN = 0x{:x}, LENGTH = 0x{:x}
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

// Our dev-mode configuration. In this case we're not being loaded by the
// factory bootloader, we're just being flashed with openocd or similar, so we
// use actual flash. The dev mode configuration has more flash and RAM available
// to it, which allows us to put heaps of debug logging in.
#[cfg(all(feature = "dev", not(feature = "stage1")))]
fn memory_x_contents() -> &'static str {
    r#"MEMORY
{
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K
    RAM  (xrw) : ORIGIN = 0x20000400, LENGTH = 9K
}
"#
}

fn write_memory_x() {
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

fn write_compressed_gateware() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    let verilog_file = "gateware/hps_tester.v";
    let pdc_file = "gateware/hps.pdc";
    let json_file = out_dir.join("hps_test_gateware.json");
    let fasm_file = out_dir.join("hps_test_gateware.fasm");
    let bit_file = out_dir.join("hps_test_gateware.bit");
    let compressed_gateware = out_dir.join("compressed_gateware.bit");

    if compressed_gateware.exists() {
        if let (Ok(compressed_meta), Ok(verilog_meta), Ok(pdc_meta)) = (
            std::fs::metadata(&compressed_gateware).and_then(|m| m.modified()),
            std::fs::metadata(verilog_file).and_then(|m| m.modified()),
            std::fs::metadata(pdc_file).and_then(|m| m.modified()),
        ) {
            if compressed_meta > std::cmp::max(verilog_meta, pdc_meta) {
                // Our final output file is strictly newer than both of our
                // input files. Skip building.
                return;
            }
        }
    }

    let mut yosys_commands: OsString = "synth_nexus -flatten -top top -json ".into();
    yosys_commands.push(&json_file);
    run(Command::new("yosys")
        .arg("-p")
        .arg(yosys_commands)
        .arg(verilog_file));

    run(Command::new("nextpnr-nexus")
        .args(["--device", "LIFCL-17-8UWG72C"])
        .args(["--pdc", pdc_file])
        .arg("--json")
        .arg(&json_file)
        .arg("--fasm")
        .arg(&fasm_file));

    run(Command::new("prjoxide")
        .arg("pack")
        .arg(&fasm_file)
        .arg(&bit_file));

    let bytes = std::fs::read(bit_file).unwrap();
    let mut compressed = Vec::new();
    gateware_compress::compress(&bytes, |byte| compressed.push(byte));
    let decompressed: Vec<u8> = gateware_compress::Decompressor::new(&compressed).collect();
    if decompressed != bytes {
        panic!("Decompressed gateware bytes differ from original!");
    }
    std::fs::write(compressed_gateware, &compressed).unwrap();
}

/// Runs `command`. Exits with a non-zero status if the command failed to run or
/// returned a non-zero exit code.
fn run(command: &mut Command) {
    // Prepend our bin directory to the path. This allows building without our
    // installed bin directory already being on the path.
    command.env(
        "PATH",
        format!(
            "../../third_party/installed/bin:{}",
            std::env::var("PATH").expect("PATH must be set")
        ),
    );
    let status = command.status();
    if !status
        .as_ref()
        .map(|status| status.success())
        .unwrap_or(false)
    {
        eprintln!("Failed to execute {:?}\nStatus: {:?}", command, status);
        std::process::exit(1);
    }
}
