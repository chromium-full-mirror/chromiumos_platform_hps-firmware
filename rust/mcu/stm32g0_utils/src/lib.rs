// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

use hal::gpio::OpenDrain;
use hal::gpio::Output;
use hal::prelude::OutputPin;

/// Note, there is a discussion at
/// https://github.com/stm32-rs/stm32g0xx-hal/issues/93 about adding something
/// like this to the HAL.
pub trait IntoOpenDrainOutputHigh {
    type Output;

    /// Converts `self` into an open-drain pin with the initial value set high,
    /// avoiding the pin ever being driven low.
    fn into_open_drain_output_high(self) -> Self::Output;
}

macro_rules! open_drain_pin {
    ($($p:tt)*) => {
        impl<MODE> IntoOpenDrainOutputHigh for $($p)*<MODE> {
            type Output = $($p)*<Output<OpenDrain>>;

            fn into_open_drain_output_high(self) -> Self::Output {
                // Pretend that we already have an output pin so that we can set
                // it high before we make it an output pin. Ordering is
                // important. If we set it as an output pin before we set it
                // high, then we'd get a glitch on the pin as it was briefly
                // driven low. If we're a peripheral on an I2C bus, this glitch
                // can make the I2C controller think that there's another
                // controller, which can cause problems.

                // Safety: The pin type is a zero-sized-type, so has no internal
                // state that we could get wrong.
                let mut fake_output: Self::Output = unsafe { core::mem::transmute(()) };
                let _ = fake_output.set_high();
                // Actually change the pin to an output pin.
                self.into_open_drain_output()
            }
        }
    };
}

// We only implement the trait for the pins that we actually need it for.

open_drain_pin!(hal::gpio::gpioa::PA11);
open_drain_pin!(hal::gpio::gpioa::PA12);
open_drain_pin!(hal::gpio::gpiob::PB6);
open_drain_pin!(hal::gpio::gpiob::PB7);
