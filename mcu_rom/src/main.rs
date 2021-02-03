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
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::{pre_init, entry, exception, ExceptionFrame};
use cortex_m_semihosting::*;//hio;

use hal::{delay::Delay, prelude::*, stm32};
use hal::serial::BasicConfig;


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
    let p = stm32::Peripherals::take().expect("No peripherals");
    let cp = Peripherals::take().expect("No peripherals");
    let mut rcc = p.RCC.constrain();

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();

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

    loop {
        led.toggle().unwrap();
        delay.delay(150.ms());
    }
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
