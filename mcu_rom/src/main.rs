#![no_main]
#![no_std]

#![feature(maybe_uninit_ref)]

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
use cortex_m::{asm, interrupt};
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::{pre_init, entry, exception, ExceptionFrame};
use cortex_m_semihosting::{hio, hprintln};

use hal::{prelude::*, stm32};
use hal::rcc::{self, PllConfig};
use hal::serial::{Serial, BasicConfig};

use hmac_sha256::Hash;

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

struct Usart {
    serial: Serial<hal::stm32::USART2, BasicConfig>,
}

impl ufmt_write::uWrite for Usart {
    type Error = ();

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        use core::fmt::Write;
        self.serial.write_str(s).map_err(drop)
    }
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
    pub static _slot_end: *const u32;
}

fn loader(usart: &mut Usart) -> Result<(), ()> {
    let magic = unsafe{ _slot_start.magic };
    if magic == 0xC03FEFE {
        uwriteln!(usart, "Found stage 1\r").map_err(drop)?;
        unsafe{
        uwriteln!(usart, " [[stage 1]] >> version={}\r", _slot_start.version).map_err(drop)?;
        uwriteln!(usart, " [[stage 1]] >> should I jmp to vector={:#?}\r", _slot_start.vector_addr).map_err(drop)?;
        uwriteln!(usart, " [[stage 1]] >> git hash build={}?\r", _slot_start.git_hash).map_err(drop)?;
        uwriteln!(usart, " [[stage 1]] >> sig[0]={}\r", _slot_start.sig[0]).map_err(drop)?;
        let sp: *const u32 = ((_slot_start.vector_addr as u32)-4) as *const u32;
        cortex_m::asm::bootload(sp);
        }
    }
    uwriteln!(usart, "Could not find stage 1").map_err(drop)
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
        match usart.serial.read(){
            Ok(b) => {
                if b == b'Y' {
                    uwriteln!(usart, " **starting..\r").map_err(drop)?;
                    break;
                } else {
                    uwriteln!(usart, " try again.. [Y]\r").map_err(drop)?;
                }
            },
            Err(_) => {},
        }
    }
    let _ = led.set_low();

    loader(&mut usart).map_err(drop)?;

    loop {}
}

//TODO(dml,quasisec): Implement uDebug for ExceptionFrame
//#[exception]
//fn HardFault(ef: &ExceptionFrame) -> ! {
//    if let Ok(mut hstdout) = hio::hstdout() {
//        uwriteln!(hstdout, "{:#?}", ef).ok();
//    }
//
//    loop {}
//}

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

fn stage1_digest() -> [u8; 32] {
    use core::mem;
    let hdr_offset: usize = mem::size_of::<ImageHeader>();
    let start: usize = 0x08002000 + hdr_offset;//(&_slot_start as *const usize) as u32;
    let   end: usize = 0x08004000;//unsafe{ _slot_end as u32 };
    const PAGE_SZ: usize = 4 * 256 * 2;
    let mut hasher = Hash::new();
    for offset in ((0)..(end-start)).step_by(PAGE_SZ) { // page flash by 2K steps.
        let region = (start + offset) as *const [u8; PAGE_SZ];
        unsafe{ hasher.update(*region) }; // extend hash a sector at a time..
    }
    hasher.finalize()
}
