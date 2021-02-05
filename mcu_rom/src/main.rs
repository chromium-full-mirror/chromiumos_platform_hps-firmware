#![no_main]
#![no_std]


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
use cortex_m_rt::{entry, exception, ExceptionFrame};
use cortex_m_semihosting::*;//hio;

use hal::{delay::Delay, prelude::*, stm32};
use hal::serial::BasicConfig;


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
