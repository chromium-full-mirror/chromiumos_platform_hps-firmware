// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_main]
#![no_std]

use cortex_m_rt::entry;
use hal::prelude::*;
use hal::stm32;

use stm32g0 as _;

#[cfg(feature = "dev")]
use panic_rtt_target as _;

#[cfg(feature = "dev")]
use rtt_target::rprintln;

#[cfg(not(feature = "dev"))]
use panic_halt as _;

// If we're in non-dev mode, then all uses of rprintln dissapear. We use
// format_args so that we don't get warnings about arguments to rprintln being
// unused.
#[cfg(not(feature = "dev"))]
macro_rules! rprintln {
    ($($x:tt)*) => {
        format_args!($($x)*);
    };
}

#[entry]
fn main() -> ! {
    #[cfg(feature = "dev")]
    {
        rtt_target::rtt_init_print!();
    }

    let p = stm32::Peripherals::take().unwrap();
    // Reset system clock source to the HSI. The factory bootloader (which we
    // are run from without a CPU reset), will have set the clock source to the
    // PLL. If we try to configure the PLL without first switching back to the
    // HSI, then we'll go into an infinite loop while we wait for the PLL to
    // disable, which it will never do, since it's being used.
    p.RCC.cfgr.write(|w| w);
    p.RCC
        .freeze(hal::rcc::Config::pll().pll_cfg(hal::rcc::PllConfig::default()));

    rprintln!("HPS tester starting");

    // TODO: Insert some testing code here

    rprintln!("Test complete");

    // Force reentry into system bootloader, but only in non-dev mode, since it
    // would be annoying to have that happen if dev mode.
    #[cfg(not(feature = "dev"))]
    {
        p.SYSCFG_VREFBUF
            .cfgr1
            .modify(|_, w| unsafe { w.mem_mode().bits(1) });
        cortex_m::peripheral::SCB::sys_reset();
    }
    #[cfg(feature = "dev")]
    loop {}
}
