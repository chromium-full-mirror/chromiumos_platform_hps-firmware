#![no_main]
#![no_std]

#![feature(maybe_uninit_ref)]
#![feature(alloc_error_handler)]

extern crate alloc;
use alloc_cortex_m::CortexMHeap;

use core::alloc::Layout;
use core::fmt::Write;

// pick a panicking behavior
#[allow(unused)]

// dev profile: easier to debug panics; can put a breakpoint on `rust_begin_unwind`
#[cfg(debug_assertions)]
use panic_halt as _;
// release profile: minimize the binary size of the application (requires nightly?)
#[cfg(not(debug_assertions))]
use panic_abort as _;

// Reports panic messages to the host stderr using semihosting; requires a debugger
// NOTE to use this you need to uncomment the `panic-semihosting` dependency in Cargo.toml
// use panic_semihosting as _;

extern crate stm32g0xx_hal as hal;
use cortex_m::{asm, interrupt};
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::{pre_init, entry, exception, ExceptionFrame};
use cortex_m_semihosting::*;//hio;

use hal::{delay::Delay, prelude::*, stm32};
use hal::rcc::{self, PllConfig};
use hal::spi;
use hal::serial::BasicConfig;

use spi_memory::{prelude::*, series25::Flash};
use hmac_sha256::*;


#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();


use core::mem::MaybeUninit;

#[macro_use]
extern crate bitflags;

bitflags! {
    struct TransientNVMemFlags: u32 {
        const DFU = 0b00000001;
        const SLOT_A = 0b00000010;
        const SLOT_B = 0b00000100;
    }
}
pub struct TransientNVMem {
    magic: u32,
    count: u8,
    flags: TransientNVMemFlags,
}

#[link_section = ".uninit.BOOT0STATE"]
static mut BOOT0STATE: MaybeUninit<TransientNVMem> = MaybeUninit::uninit();

// summon the C devil, open a portal to Jekkad
#[pre_init]
unsafe fn init_boot0_state() {
    // cold on, previous state unknown!
    let magic = BOOT0STATE.assume_init_ref().magic;
    if magic != 0xC0FFEE {
        BOOT0STATE = MaybeUninit::zeroed();
        BOOT0STATE.assume_init_mut().magic = 0xC0FFEE;
    }
    BOOT0STATE.assume_init_mut().count = BOOT0STATE.assume_init_ref().count + 1;
}


#[entry]
fn main() -> ! {
    // Initialize the allocator BEFORE you use it
    let start = cortex_m_rt::heap_start() as usize;
    let size = 2048; // in bytes
    unsafe { ALLOCATOR.init(start, size) }

    let p = stm32::Peripherals::take().expect("No peripherals");
    let cp = Peripherals::take().expect("No peripherals");

    // Configure APB bus clock to 48MHz, as we requires 3Mbps SPI
    //let pll_cfg = PllConfig::with_hsi(4, 24, 2);
    let pll_cfg = PllConfig::default(); // (1, 8, 2) -> ((16MHz/1) * 8)/2 = 64Mhz (max APB bus speed).
    let rcc_cfg = rcc::Config::pll().pll_cfg(pll_cfg);
    let mut rcc = p.RCC.freeze(rcc_cfg);

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();
    // configure PA4 as o/p
    let spi_cs = gpioa.pa4.into_open_drain_output();
    // configure PB4 as o/p
    let gpiob = p.GPIOB.split(&mut rcc); // ::gpio::GpioExt
    let mut fpga_pw_gate = gpiob.pb4.into_push_pull_output();
    // configure PC14 as i/p
    let gpioc = p.GPIOC.split(&mut rcc); // ::gpio::GpioExt
    let fpga_done = gpioc.pc14.into_floating_input();
    // configure PC15 as o/p
    let mut fpga_prog = gpioc.pc15.into_open_drain_output();

    // the art of implementing a stm32 bootloader in rust.
    hprintln!("{}\n", banner()).unwrap();

    let mut delay = Delay::syst(cp.SYST, &rcc);
    hprintln!("init USART..").unwrap();
    let mut usart = p
        .USART2
        .usart(
            gpioa.pa2, // rx
            gpioa.pa3, // tx
            BasicConfig::default().baudrate(115200.bps()),
            &mut rcc,
        )
        .unwrap();
    writeln!(usart, "booting..\r").unwrap();
    hprintln!("USART up!").unwrap();
    let boot_count = unsafe { BOOT0STATE.assume_init_ref().count };
    writeln!(usart, "boot count = {}\r", boot_count).unwrap();

    // stop FPGA running and taking SPI flash from us.
    fpga_prog.set_low().unwrap(); // SW1 (pressed)?
    fpga_pw_gate.set_low().unwrap(); // also gates power to SPI flash!!!
    if fpga_done.is_high().unwrap() {
        writeln!(usart, "FPGA done\r").unwrap();
    } else {
        writeln!(usart, "FPGA wait\r").unwrap();
    }

    let mut spi = p.SPI1.spi(
        (gpioa.pa5, gpioa.pa6, gpioa.pa7), // (SCK, MISO, MOSI)
//        spi::MODE_0, // CPOL = 0, CPHA = 0
        spi::MODE_3, // Mode 3: CPOL = 1, (SCK=1 in idle), CPHA = 1
        4.mhz(),
        &mut rcc,
    );
    hprintln!("SPI up!, attempt to init SPI rom..").unwrap();

    delay.delay(20.ms()); // wait for SPI to quiesce.
    // XXX Here we don't expect any writes to be in progress, and the latch must also be deasserted.
    // strap WP and HOLD lines correctly or WEL wont be 0? - HOLD looks like highZ???
    // things that can cause BUSY bit to be set??
    let mut flash = Flash::init(spi, spi_cs).expect("WEL and BUSY bit should be clear"); // can causes HardFault when writes are in progess!
    hprintln!("attempt to query SPI rom..").unwrap();
    let jedec_id = flash.read_jedec_id(); // expect Identification([c8, 60, 18])
    match jedec_id {
        Ok(id) => {
            if id.mfr_code() == 0xC8 && id.device_id() == [0x60, 0x18] {
                writeln!(usart, "Found GigaDevice GD25LQ128C/D !\r").unwrap();
            }
        },
        Err(..) => {},
    }

    //flash.erase_all();

    //let addr = 0x0000AA; // 24bit start addr.
    //let mut z:[u8; 12] = [0x00; 12];
    //z[0] = 'A' as u8;
    //z[1] = 'B' as u8;
    //z[2] = 'C' as u8;
    //flash.write_bytes(addr, &mut z);

    let mut stopwatch = p.TIM2.stopwatch(&mut rcc);

    let mut hasher = Hash::new();
    let mut buf:[u8; 16*256] = [0x00; 16*256]; // 4K sector: each page is 256B and each sector is 16 pages.
    hprintln!("read SPI rom sectors..").unwrap();
    let elapsed_us = stopwatch.trace(|| {
        for addr in ((0)..(16384*1024)).step_by(16*256) { // addr += 4K [16*256] < 64K [16777216B] of flash
//        for addr in ((16*256)..(8192)).step_by(16*256) { // addr += 4K [16*256] < 64K [16777216B] of flash
//            writeln!(usart, "spi rom@{:04X?}\r", addr).unwrap();
            flash.read(addr, &mut buf).unwrap(); // first sector 0x0-0x000FFF[4095=(16 * 256 - 1)]
            hasher.update(&buf); // extend hash a sector at a time..
            led.toggle().unwrap();
        }
    });
    writeln!(usart, "spi rom@..={:02X?}\r", buf).unwrap();
    let rom_sha = hasher.finalize();
    hprintln!("Time elapsed {}us", elapsed_us.0).unwrap();

    // $ yes this is test file | head -c 16777216 > uran.rom
    // $ sudo ./flashrom -p dediprog -w uran.rom
    let valid_rom = [
        0xE7, 0xBB, 0xCC, 0x0B, 0x5E, 0x68, 0x3A, 0x16,
        0xD1, 0x5A, 0x25, 0xB0, 0xA0, 0x42, 0xA8, 0xDD,
        0xB2, 0xD4, 0xB1, 0x81, 0x1A, 0xD4, 0x47, 0xB9,
        0xF6, 0x7E, 0x94, 0x0B, 0x94, 0x82, 0x3E, 0x26
    ];

    if rom_sha != valid_rom {
        writeln!(usart, "INVALID hash!!!\r").unwrap();
        writeln!(usart, "  FOUND => rom hash={:02X?}\r", rom_sha).unwrap();
        writeln!(usart, "  starting FPGA anyway because security is annoying..\r").unwrap();
    } else {
        writeln!(usart, "FOUND VALID hash!!!\r").unwrap();
        writeln!(usart, "  FOUND => rom hash={:02X?}\r", rom_sha).unwrap();
        writeln!(usart, "  starting secure FPGA bitstream..\r").unwrap();
    }

    led.set_high().unwrap();
    writeln!(usart, "continue? [Y]\r").unwrap();
    loop {
        match usart.read(){
            Ok(b) => {
                if b == b'Y' {
                    writeln!(usart, " **starting..\r").unwrap();
                    break;
                } else {
                    writeln!(usart, " try again.. [Y]\r").unwrap();
                }
            },
            Err(_) => {},
        }
    }
    led.set_low().unwrap();

//    spi_cs.set_low().unwrap();
//    let mut data :[u8; 4] = [0x9f, 0x00, 0x00, 0x00]; // JEDEC_RDID
//    //let mut data :[u8; 4] = [0xd7, 0x00, 0x00, 0x00]; // JEDEC_RDSR
//    spi.transfer(&mut data).expect("spi xfer complete");
//    writeln!(usart, "JEDEC_RDID res={:02X?}\r", data).unwrap();
//    spi_cs.set_high().unwrap();
//    spi.release();

    // start FPGA running..
    fpga_prog.set_high().unwrap(); // SW1 (released).
    fpga_pw_gate.set_low().unwrap();
    delay.delay(200.ms()); // wait for FPGA to quiesce.
    if fpga_done.is_high().unwrap() {
        writeln!(usart, "FPGA done\r").unwrap();
    } else {
        writeln!(usart, "FPGA wait\r").unwrap();
    }

    loop {
        led.toggle().unwrap();
        delay.delay(700.ms());
    }
}

#[exception]
fn HardFault(ef: &ExceptionFrame) -> ! {
    if let Ok(mut hstdout) = hio::hstdout() {
        writeln!(hstdout, "{:#?}", ef).ok();
    }

    loop {}
}

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    asm::bkpt();
    loop {}
}

fn banner() -> &'static str {
    r#"
 C
 (\.   \      ,/)
  \(   |\     )/
  //\  | \   /\\
 (/ /\_#oo#_/\ \)
  \/\  ####  /\/
       `##'
quasisec~."#
}
