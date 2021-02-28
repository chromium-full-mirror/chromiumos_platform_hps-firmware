#![no_main]
#![no_std]
#![feature(maybe_uninit_ref)]

mod common;
use crate::common::*;

mod loader;
use crate::loader::*;

use ufmt::uwriteln;

// pick a panicking behavior
#[allow(unused)]
// dev profile: easier to debug panics; can put a breakpoint on `rust_begin_unwind`
#[cfg(debug_assertions)]
use panic_halt as _;
// release profile: minimize the binary size of the application (requires nightly?)
#[cfg(not(debug_assertions))]
use panic_abort as _;

extern crate stm32g0xx_hal as hal;
use cortex_m_rt::{entry, pre_init};

use hal::serial::BasicConfig;
use hal::{prelude::*, stm32};

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
    // TODO: Use this field
    #[allow(unused)]
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
    match inner_main() {
        Ok(()) => cortex_m::peripheral::SCB::sys_reset(),
        //Err(e) => panic!(e),
        Err(_) => loop {},
    }
}

fn inner_main() -> Result<(), ()> {
    let p = stm32::Peripherals::take().ok_or(())?;

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
    let mut fpga_prog = gpioc.pc15.into_push_pull_output();

    let raw_usart = p
        .USART2
        .usart(
            gpioa.pa2, // rx
            gpioa.pa3, // tx
            BasicConfig::default().baudrate(115_200.bps()),
            &mut rcc,
        )
        .map_err(drop)?;
    let mut usart = Usart { serial: raw_usart };
    uwriteln!(usart, "booting..\r").map_err(drop)?;

    // the art of implementing a stm32 bootloader in rust.
    #[cfg(jtag)]
    {
        use cortex_m_semihosting::hprintln;
        hprintln!("{}\n", banner()).map_err(drop)?;
    }

    let boot_count = unsafe { BOOT0STATE.assume_init_ref().count };
    uwriteln!(usart, "boot count = {}\r", boot_count).map_err(drop)?;

    // stop FPGA running and taking SPI flash from us.
    let _ = fpga_prog.set_low(); // SW1 (pressed)?
    let _ = fpga_pw_gate.set_low(); // also gates power to SPI flash!!!
    if fpga_done.is_high().unwrap() {
        uwriteln!(usart, "FPGA done\r").map_err(drop)?;
    } else {
        uwriteln!(usart, "FPGA wait\r").map_err(drop)?;
    }

    let _ = led.set_high();
    let stage1_digest = stage1_digest();
    uwriteln!(usart, "{:?}\r", stage1_digest).map_err(drop)?;
    let _ = led.set_low();

    let _ = led.set_high();
    uwriteln!(usart, "continue? [Y]\r").map_err(drop)?;
    loop {
        match usart.serial.read() {
            Ok(b) => {
                if b == b'Y' {
                    uwriteln!(usart, " **starting..\r").map_err(drop)?;
                    break;
                } else {
                    uwriteln!(usart, " try again.. [Y]\r").map_err(drop)?;
                }
            }
            Err(_) => {}
        }
    }
    let _ = led.set_low();

    loader(&mut usart).map_err(drop)?;

    loop {}
}

//TODO(dml,quasisec): Implement uDebug for ExceptionFrame
//#[exception]
//fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
//    if let Ok(mut hstdout) = cortex_m_semihosting::hio::hstdout() {
//        uwriteln!(hstdout, "{:#?}", ef).ok();
//    }
//
//    loop {}
//}

#[cfg(jtag)]
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
