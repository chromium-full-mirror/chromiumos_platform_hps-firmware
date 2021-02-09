#![no_main]
#![no_std]

#![feature(maybe_uninit_ref)]

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
use hal::serial::BasicConfig;
use hal::i2c::Config;


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

#[repr(C,packed(4))]
pub struct ImageHeader {
    magic: u32,
    version: u16,
    vector_addr: *const u32, // re-interpret
    git_hash: &'static str,
    sig: [u8; 64],
}

extern "C" {
    #[no_mangle]
    pub static _slot_start: ImageHeader;
}

fn loader() {
    let magic = unsafe{ _slot_start.magic };
    if magic == 0xC03FEFE {
        hprintln!("Found stage 1").unwrap();
        unsafe{
        hprintln!(" [[stage 1]] >> version={}", _slot_start.version).unwrap();
        hprintln!(" [[stage 1]] >> should I jmp to vector=0x{:08X?}", _slot_start.vector_addr).unwrap();
        hprintln!(" [[stage 1]] >> git hash build={}?", _slot_start.git_hash).unwrap();
        hprintln!(" [[stage 1]] >> sig[0]={}", _slot_start.sig[0]).unwrap();
        let sp: *const u32 = ((_slot_start.vector_addr as u32)-4) as *const u32;
        cortex_m::asm::bootload(sp);
        }
    } else {
        hprintln!("Could not find stage 1").unwrap();
    }
}

#[entry]
fn main() -> ! {
    let p = stm32::Peripherals::take().expect("No peripherals");
    let cp = Peripherals::take().expect("No peripherals");

    // Leave clocks as defaults and let stage 1 configure them.
    let mut rcc = p.RCC.constrain();
    rcc.disable_low_power_mode();

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();
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

    loader();

    let mut stopwatch = p.TIM2.stopwatch(&mut rcc);

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

// MCU_I2C_CAM_xxx
//    let mut i2c = p.I2C1.i2c(
//        gpiob.pb7.into_open_drain_output(), // sda
//        gpiob.pb6.into_open_drain_output(), // scl
//        Config::new(1.mhz()),
//        &mut rcc
//    );

    loop {}
}

#[exception]
fn HardFault(ef: &ExceptionFrame) -> ! {
    if let Ok(mut hstdout) = hio::hstdout() {
        writeln!(hstdout, "{:#?}", ef).ok();
    }

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
