#![no_main]
#![no_std]

#![feature(alloc_error_handler)]

#[macro_use]
extern crate alloc;

use alloc_cortex_m::CortexMHeap;
use core::alloc::Layout;
use self::alloc::vec;

use core::fmt::Write; // needed for usart TODO(dml): replace with ufmt-write

use panic_abort as _;

extern crate stm32g0xx_hal as hal;
use cortex_m::{asm};
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::{entry};
use cortex_m_semihosting::*;

use hal::{delay::Delay, prelude::*, stm32};
use hal::spi;
use hal::rcc::{self, PllConfig};
use hal::serial::BasicConfig;

use spi_memory::{prelude::*, series25::Flash};
use hmac_sha256::*;

use git_version::git_version;

#[repr(C, packed(4))]
pub struct ImageHeader {
    magic: u32,
    version: u16,
    vector_addr: &'static u32,
    git_hash: &'static str,
    sig: [u8; 64],
}

extern "C" {
    pub static __RESET_VECTOR: u32;
}

#[link_section = ".image_hdr"]
#[no_mangle]
pub static __IMAGE_HDR: ImageHeader = ImageHeader{
    magic: 0xC03FEFE,
    version: 1,
    vector_addr: unsafe{ &__RESET_VECTOR },
    git_hash: git_version!(),
    sig: [0;64], // constructed with padded zeros for unsigned image.
};


#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

const HEAP_SIZE: usize = 2048; // in bytes


#[entry]
fn main() -> ! {
    match inner_main() {
        Ok(()) => cortex_m::peripheral::SCB::sys_reset(),
        //Err(e) => panic!(e),
        Err(_) => loop {},
    }
}

fn build_hash() -> &'static str {
    __IMAGE_HDR.git_hash
}

fn inner_main() -> Result<(), ()> {
    // Initialize the allocator BEFORE you use it
    let start = cortex_m_rt::heap_start() as usize;
    unsafe { ALLOCATOR.init(start, HEAP_SIZE) }

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
    let mut spi_cs = gpioa.pa4.into_push_pull_output();

    led.set_high().map_err(drop)?;

    let mut spi = p.SPI1.spi(
        (gpioa.pa5, gpioa.pa6, gpioa.pa7), // (SCK, MISO, MOSI)
//        spi::MODE_0, // CPOL = 0, CPHA = 0
        spi::MODE_3, // Mode 3: CPOL = 1, (SCK=1 in idle), CPHA = 1
        1.mhz(),
        &mut rcc,
    );
//    hprintln!("SPI up!, attempt to init SPI rom..").map_err(drop)?;

    let mut usart = p
        .USART2
        .usart(
            gpioa.pa2, // rx
            gpioa.pa3, // tx
            BasicConfig::default().baudrate(115_200.bps()),
            &mut rcc,
        )
        .map_err(drop)?;
    writeln!(usart, "booting stage1 {:?}", build_hash()).map_err(drop)?;

    let cp = Peripherals::take().ok_or(())?;
    let mut delay = Delay::syst(cp.SYST, &rcc);
    delay.delay(20.ms()); // wait for SPI to quiesce.

    let mut flash = Flash::init(spi, spi_cs).map_err(drop)?;//.expect("WEL and BUSY bit should be clear"); // can causes HardFault when writes are in progess!
    //hprintln!("attempt to query SPI rom..").map_err(drop)?;
    let jedec_id = flash.read_jedec_id(); // expect Identification([c8, 60, 18])
    match jedec_id {
        Ok(id) => {
            if id.mfr_code() == 0xC8 && id.device_id() == [0x60, 0x18] {
                hprintln!("Found GigaDevice GD25LQ128C/D !\r").ok();
            }
        },
        Err(..) => {},
    }

    let mut stopwatch = p.TIM2.stopwatch(&mut rcc);

    let mut hasher = Hash::new();
//    let mut buf = vec![0; 16*256];
    let mut buf:[u8; 16*256] = [0x00; 16*256]; // 4K sector: each page is 256B and each sector is 16 pages.
    let elapsed_us = stopwatch.trace(|| {
        for addr in ((0)..(16384*1024)).step_by(16*256) { // addr += 4K [16*256] < 64K [16777216B] of flash
//        for addr in ((16*256)..(8192)).step_by(16*256) { // addr += 4K [16*256] < 64K [16777216B] of flash
//            writeln!(usart, "spi rom@{:04X?}\r", addr).unwrap();
            flash.read(addr, &mut buf).map_err(drop).unwrap(); // first sector 0x0-0x000FFF[4095=(16 * 256 - 1)]
//            hasher.update(&buf); // extend hash a sector at a time..
            led.toggle().map_err(drop).unwrap();
        }
    });
//    hprintln!("spi rom@..={:02X?}\r", buf).map_err(drop)?;
//    let rom_sha = hasher.finalize();
//    hprintln!("Time elapsed {}us", elapsed_us.0).map_err(drop)?;

    loop {}
}

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    asm::bkpt();
    loop {}
}
