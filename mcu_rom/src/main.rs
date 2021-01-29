#![no_main]
#![no_std]

#[allow(unused)]
use panic_semihosting as _;

extern crate stm32g0xx_hal as hal;
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::entry;

use hal::{delay::Delay, prelude::*, stm32};

#[entry]
fn main() -> ! {
    let p = stm32::Peripherals::take().expect("No peripherals");
    let cp = Peripherals::take().expect("No peripherals");
    let mut rcc = p.RCC.constrain();

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();

    let mut delay = Delay::syst(cp.SYST, &rcc);

    loop {
        led.toggle().unwrap();
        delay.delay(150.ms());
    }
    panic!("TODO(quasisec): simply write the rest of the bootloader.");
}
