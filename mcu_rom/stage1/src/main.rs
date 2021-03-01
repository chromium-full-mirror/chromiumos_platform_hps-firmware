// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]
#![feature(alloc_error_handler)]

use panic_reset as _;

extern crate stm32g0xx_hal as hal;
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::entry;

use hal::rcc::{self, PllConfig};
use hal::serial::BasicConfig;
use hal::spi;
use hal::{delay::Delay, prelude::*, stm32};

use hmac_sha256::Hash;
use spi_memory::{prelude::*, series25::Flash};

use ufmt::uwriteln;

mod usart;
use crate::usart::*;

mod header;
use crate::header::*;

mod heap;
use crate::heap::*;

#[entry]
fn main() -> ! {
    match inner_main() {
        Ok(()) => cortex_m::peripheral::SCB::sys_reset(),
        //Err(e) => panic!(e),
        Err(_) => loop {},
    }
}

fn inner_main() -> Result<(), ()> {
    // Initialize the allocator BEFORE you use it
    init_heap();

    let p = stm32::Peripherals::take().ok_or(())?;

    // APB bus clocks configured in stage 0. Just inherit system state.
    // Configure APB bus clocks, as we requires full speed SPI
    let pll_cfg = PllConfig::default(); // (1, 8, 2) -> ((16MHz/1) * 8)/2 = 64Mhz (max APB bus speed).
    let rcc_cfg = rcc::Config::pll().pll_cfg(pll_cfg);
    let mut rcc = p.RCC.freeze(rcc_cfg);

    // configure PA1 as o/p
    let gpioa = p.GPIOA.split(&mut rcc); // ::gpio::GpioExt
    let mut led = gpioa.pa1.into_push_pull_output();
    // configure PA4 as o/p
    //    let spi_cs = {
    //        let mut cs = gpioa.pa4.into_open_drain_output();
    //        //cs.set_high().unwrap(); // de-select
    //        cs.set_low().unwrap(); // select
    //        cs
    //    };
    let spi_cs = gpioa.pa4.into_push_pull_output();

    led.set_high().map_err(drop)?;

    let spi = p.SPI1.spi(
        (gpioa.pa5, gpioa.pa6, gpioa.pa7), // (SCK, MISO, MOSI)
        //        spi::MODE_0, // CPOL = 0, CPHA = 0
        spi::MODE_3, // Mode 3: CPOL = 1, (SCK=1 in idle), CPHA = 1
        1.mhz(),
        &mut rcc,
    );

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
    uwriteln!(usart, "booting stage1 {}\r", build_hash()).map_err(drop)?;

    let cp = Peripherals::take().ok_or(())?;
    let mut delay = Delay::syst(cp.SYST, &rcc);
    delay.delay(20.ms()); // wait for SPI to quiesce.

    let mut flash = Flash::init(spi, spi_cs).map_err(drop)?; //.expect("WEL and BUSY bit should be clear"); // can causes HardFault when writes are in progess!
    let jedec_id = flash.read_jedec_id(); // expect Identification([c8, 60, 18])
    match jedec_id {
        Ok(id) => {
            if id.mfr_code() == 0xC8 && id.device_id() == [0x60, 0x18] {
                uwriteln!(usart, "Found GigaDevice GD25LQ128C/D !\r").map_err(drop)?;
            }
        }
        Err(..) => {}
    }

    let stopwatch = p.TIM2.stopwatch(&mut rcc);

    let mut hasher = Hash::new();
    //    let mut buf = vec![0; 16*256];
    let mut buf: [u8; 16 * 256] = [0x00; 16 * 256]; // 4K sector: each page is 256B and each sector is 16 pages.
    let elapsed_us = stopwatch.trace(|| {
        for addr in ((0)..(16384 * 1024)).step_by(16 * 256) {
            // addr += 4K [16*256] < 64K [16777216B] of flash
            //        for addr in ((16*256)..(8192)).step_by(16*256) { // addr += 4K [16*256] < 64K [16777216B] of flash
            //            writeln!(usart, "spi rom@{:04X?}\r", addr).unwrap();
            //TODO(quasisec): We should handle the errors back from read, ignore for now.
            let _ = flash.read(addr, &mut buf); // first sector 0x0-0x000FFF[4095=(16 * 256 - 1)]
            hasher.update(&buf); // extend hash a sector at a time..
            let _ = led.toggle();
        }
    });
    // TODO: Make use of rom_sha
    let _rom_sha = hasher.finalize();
    uwriteln!(usart, "Time elapsed {}us\r", elapsed_us.0).map_err(drop)?;

    // configure PB4 as o/p
    let gpiob = p.GPIOB.split(&mut rcc); // ::gpio::GpioExt
    let mut fpga_pw_gate = gpiob.pb4.into_push_pull_output();
    // configure PC14 as i/p
    let gpioc = p.GPIOC.split(&mut rcc); // ::gpio::GpioExt
    // TODO: Make use of fpga_done.
    let _fpga_done = gpioc.pc14.into_floating_input();
    // configure PC15 as o/p
    let mut fpga_prog = gpioc.pc15.into_push_pull_output();

    // start FPGA running..
    fpga_prog.set_high().map_err(drop)?; // SW1 (released).
    fpga_pw_gate.set_low().map_err(drop)?;
    delay.delay(200.ms()); // wait for FPGA to quiesce.

    loop {
        led.toggle().map_err(drop)?;
        delay.delay(700.ms());
    }
}
