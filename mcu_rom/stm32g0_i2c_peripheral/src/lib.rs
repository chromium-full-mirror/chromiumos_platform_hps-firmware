// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::prelude::*;
use hal::rcc::Rcc;
use hal::stm32;
use hal::stm32::I2C1;
use i2c_peripheral::I2cError;
use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;

#[derive(Copy, Clone, Default)]
pub struct Config {
    /// Whether to enable i2c clock stretching.
    pub clock_stretching: bool,
}

#[non_exhaustive]
pub struct Stm32g0I2cPeripheral {
    // TODO: Make generic over hardware
    i2c1: I2C1,
}

impl Stm32g0I2cPeripheral {
    /// Initializes the I2C interface with the host (I2C1).
    pub fn new(
        i2c1: I2C1,
        rcc: &mut Rcc,
        sda: impl SDAPin<I2C1>,
        scl: impl SCLPin<I2C1>,
        // TODO: Consider moving address into Config.
        address: u8,
        config: Config,
    ) -> Self {
        let i2c = i2c1.i2c(sda, scl, hal::i2c::Config::new(400.khz()), rcc);

        // un-own p.I2C1 block to tap registers
        let (ii2c, _, _) = i2c.release();

        // The I2C unit needs to be disabled while we configure it
        ii2c.cr1.modify(|_, w| w.pe().clear_bit());
        // Enable the kinds of I2C interrupts that we want to handle
        ii2c.cr1.modify(|_, w| w.addrie().set_bit());
        ii2c.cr1.modify(|_, w| w.rxie().set_bit());
        ii2c.cr1.modify(|_, w| w.txie().set_bit());
        ii2c.cr1.modify(|_, w| w.stopie().set_bit());
        ii2c.cr1.modify(|_, w| w.errie().set_bit());
        // Disable clock stretching.
        ii2c.cr1
            .modify(|_, w| w.nostretch().bit(!config.clock_stretching));
        // Enable the I2C unit
        ii2c.cr1.modify(|_, w| w.pe().set_bit());

        // Set our I2C address. oa1en need to be clear while we set the address.
        ii2c.oar1.modify(|_, w| w.oa1en().clear_bit());
        ii2c.oar1.modify(|_, w| w.oa1mode().clear_bit()); // 7bit addr
        ii2c.oar1
            .modify(|_, w| unsafe { w.oa1_7_1().bits(address) });
        // Enable automatic ACK of our address.
        ii2c.oar1.modify(|_, w| w.oa1en().set_bit());

        Self { i2c1: ii2c }
    }

    pub fn enable_interupts(&mut self) {
        stm32::NVIC::unpend(hal::stm32::Interrupt::I2C1);
        unsafe { stm32::NVIC::unmask(hal::stm32::Interrupt::I2C1) };
    }

    pub fn disable_interrupts(&mut self) {
        stm32::NVIC::unpend(hal::stm32::Interrupt::I2C1);
        stm32::NVIC::mask(hal::stm32::Interrupt::I2C1);
        self.i2c1.cr1.modify(|_, w| w.pe().clear_bit());
    }

    pub fn set_i2c_enabled(&mut self, enabled: bool) {
        self.i2c1.oar1.modify(|_, w| w.oa1en().bit(enabled));
    }
}

impl Drop for Stm32g0I2cPeripheral {
    fn drop(&mut self) {
        self.disable_interrupts();
        // Disable I2C
        self.i2c1.cr1.modify(|_, w| w.pe().clear_bit());
    }
}

impl I2cPeripheral for Stm32g0I2cPeripheral {
    fn next_event(&mut self) -> I2cEvent {
        let isr = self.i2c1.isr.read();
        if isr.txis().bit_is_set() {
            return I2cEvent::NeedByte;
        }
        if isr.rxne().bit_is_set() {
            return I2cEvent::ByteReceived(self.i2c1.rxdr.read().bits() as u8);
        }
        if isr.addr().bit_is_set() {
            self.i2c1.icr.write(|w| w.addrcf().set_bit());
            if isr.dir().bit_is_set() {
                return I2cEvent::StartRead;
            } else {
                return I2cEvent::StartWrite;
            }
        }
        if isr.stopf().bit_is_set() {
            self.i2c1.icr.write(|w| w.stopcf().set_bit());
            // Setting txe will clear any existing byte already stored into
            // txrd. This is important if the previous read was terminated early
            // by the controller.
            self.i2c1.isr.write(|w| w.txe().set_bit());
            return I2cEvent::Stop;
        }
        if isr.ovr().bit_is_set() {
            self.i2c1.icr.write(|w| w.ovrcf().set_bit());
            return I2cEvent::Error(if isr.dir().bit_is_set() {
                I2cError::Underrun
            } else {
                I2cError::Overrun
            });
        }
        if isr.berr().bit_is_set() {
            self.i2c1.icr.write(|w| w.berrcf().set_bit());
            return I2cEvent::Error(I2cError::BusError);
        }
        I2cEvent::None
    }

    fn write_byte(&mut self, byte: u8) {
        self.i2c1.txdr.write(|w| unsafe { w.txdata().bits(byte) });
    }
}
