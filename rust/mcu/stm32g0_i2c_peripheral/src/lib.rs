// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

use core::ops::Deref;
use hal::i2c::SCLPin;
use hal::i2c::SDAPin;
use hal::rcc::Rcc;
use hal::stm32;
use hal::time::Hertz;
use i2c_peripheral::I2cError;
use i2c_peripheral::I2cEvent;
use i2c_peripheral::I2cPeripheral;

#[derive(Copy, Clone)]
#[non_exhaustive]
pub struct Config {
    /// Whether to enable i2c clock stretching.
    pub clock_stretching: bool,
    address: Address,
    timing: Timing,
}

#[derive(Copy, Clone)]
pub struct Address {
    /// A 7 bit address. Private to allow future support for 10 bit addresses.
    address: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct Timing {
    raw_timing_bits: u32,
}

/// A bit of hardware within the STM32 that controls the I2C bus. e.g. I2C1,
/// I2C2 etc.
pub trait I2cBusController: Deref<Target = stm32::i2c1::RegisterBlock> {
    fn enable_power(rcc: &mut Rcc);
    fn reset_peripheral(rcc: &mut Rcc);
    fn interrupt() -> hal::stm32::Interrupt;
}

#[non_exhaustive]
pub struct Stm32g0I2cPeripheral<I2C: I2cBusController> {
    i2c: I2C,
}

impl Config {
    pub const fn new(address: Address, timing: Timing) -> Config {
        Config {
            clock_stretching: true,
            address,
            timing,
        }
    }
}

impl Address {
    pub const fn new_7bit(address: u8) -> Self {
        Self { address }
    }
}

impl Timing {
    /// Compute timing bits based on a requested I2C bus speed, taking into
    /// account the current system clock configuration.
    pub fn from_speed(rcc: &Rcc, speed: Hertz) -> Self {
        Self {
            raw_timing_bits: timing(rcc, speed),
        }
    }

    /// Use raw timing bits computed elsewhere. There are various external
    /// calculators you can use to compute these, e.g. STM32CubeMX.
    pub const fn from_bits(raw_timing_bits: u32) -> Self {
        Self { raw_timing_bits }
    }
}

impl<I2C: I2cBusController> Stm32g0I2cPeripheral<I2C> {
    /// Initializes the I2C interface as an I2C peripheral.
    pub fn new(
        i2c: I2C,
        rcc: &mut Rcc,
        sda: impl SDAPin<I2C>,
        scl: impl SCLPin<I2C>,
        config: Config,
    ) -> Self {
        // Configure pins.
        sda.setup();
        scl.setup();

        let mut peripheral = Self { i2c };
        peripheral.reinitialize(rcc, config);
        peripheral
    }

    /// Reinitializes the peripheral. Can be used if you've changed the clock
    /// configuration.
    pub fn reinitialize(&mut self, rcc: &mut Rcc, config: Config) {
        I2C::enable_power(rcc);
        I2C::reset_peripheral(rcc);

        // The I2C unit needs to be disabled while we configure it. Set CR1 to
        // default value.
        self.i2c.cr1.write(|w| w);
        // Configure timing.
        self.i2c
            .timingr
            .write(|w| unsafe { w.bits(config.timing.raw_timing_bits) });
        self.i2c.cr1.modify(|_, w| {
            // Get an interrupt when we're addressed.
            w.addrie().set_bit();
            // Get an interrupt when we receive a byte.
            w.rxie().set_bit();
            // Get an interrupt when our transmit buffer is empty.
            w.txie().set_bit();
            // Get an interrupt when we receive a STOP signal.
            w.stopie().set_bit();
            // Get an interrupt when there's an error.
            w.errie().set_bit();
            // Enable or disable clock stretching.
            w.nostretch().bit(!config.clock_stretching)
        });
        // Enable the I2C unit
        self.i2c.cr1.modify(|_, w| w.pe().set_bit());

        // Set our I2C address. oa1en need to be clear while we set the address.
        self.i2c.oar1.modify(|_, w| w.oa1en().clear_bit());
        self.i2c.oar1.modify(|_, w| w.oa1mode().clear_bit()); // 7bit addr
        self.i2c
            .oar1
            .modify(|_, w| unsafe { w.oa1_7_1().bits(config.address.address) });
        // Enable automatic ACK of our address.
        self.i2c.oar1.modify(|_, w| w.oa1en().set_bit());
    }

    pub fn enable_interupts(&mut self) {
        stm32::NVIC::unpend(I2C::interrupt());
        unsafe { stm32::NVIC::unmask(I2C::interrupt()) };
    }

    pub fn disable_interrupts(&mut self) {
        stm32::NVIC::unpend(I2C::interrupt());
        stm32::NVIC::mask(I2C::interrupt());
        self.i2c.cr1.modify(|_, w| w.pe().clear_bit());
    }

    pub fn set_i2c_enabled(&mut self, enabled: bool) {
        self.i2c.oar1.modify(|_, w| w.oa1en().bit(enabled));
    }
}

impl<I2C: I2cBusController> Drop for Stm32g0I2cPeripheral<I2C> {
    fn drop(&mut self) {
        self.disable_interrupts();
        // Disable I2C
        self.i2c.cr1.modify(|_, w| w.pe().clear_bit());
    }
}

impl<I2C: I2cBusController> I2cPeripheral for Stm32g0I2cPeripheral<I2C> {
    fn next_event(&mut self) -> nb::Result<I2cEvent, I2cError> {
        let isr = self.i2c.isr.read();
        if isr.txis().bit_is_set() {
            return Ok(I2cEvent::NeedByte);
        }
        if isr.rxne().bit_is_set() {
            return Ok(I2cEvent::ByteReceived(self.i2c.rxdr.read().bits() as u8));
        }
        if isr.addr().bit_is_set() {
            self.i2c.icr.write(|w| w.addrcf().set_bit());
            if isr.dir().bit_is_set() {
                return Ok(I2cEvent::StartRead);
            } else {
                return Ok(I2cEvent::StartWrite);
            }
        }
        if isr.stopf().bit_is_set() {
            self.i2c.icr.write(|w| w.stopcf().set_bit());
            // Setting txe will clear any existing byte already stored into
            // txrd. This is important if the previous read was terminated early
            // by the controller.
            self.i2c.isr.write(|w| w.txe().set_bit());
            return Ok(I2cEvent::Stop);
        }
        if isr.ovr().bit_is_set() {
            self.i2c.icr.write(|w| w.ovrcf().set_bit());
            if isr.dir().bit_is_set() {
                return Err(nb::Error::Other(I2cError::Underrun));
            } else {
                return Err(nb::Error::Other(I2cError::Overrun));
            };
        }
        if isr.berr().bit_is_set() {
            self.i2c.icr.write(|w| w.berrcf().set_bit());
            return Err(nb::Error::Other(I2cError::BusError));
        }
        Err(nb::Error::WouldBlock)
    }

    fn write_byte(&mut self, byte: u8) {
        self.i2c.txdr.write(|w| unsafe { w.txdata().bits(byte) });
    }

    fn reset(&mut self) {
        self.i2c.cr1.modify(|_, w| w.pe().clear_bit());
        self.i2c.cr1.modify(|_, w| w.pe().set_bit());
    }
}

/// Returns the timing configuration in order to run i2c at the specified speed.
fn timing(rcc: &Rcc, i2c_speed: Hertz) -> u32 {
    // The examples in RM0444 section 32.4.11 divide the APB clock by different
    // values in order to achieve 8MHz, so we do the same.
    const TARGET_CLOCK: u32 = 8_000_000;

    // RM0444 says that the period of SCL is longer than scl_low + scl_high, but
    // it doesn't say by how much. It's likely that the exact value depends on a
    // range of factors such as rise and fall times as well as delays introduced
    // by the analog and digital filters. We don't currently take any of that
    // into account though. This value is calculated based on the examples in
    // section 32.4.11 such that `t(SCL) = t(SCL_LOW) + t(SCL_HIGH)
    // + SCL_READ_DELAY_NS`.
    const SCL_READ_DELAY_NS: u32 = 750;

    // The low and high timings for SCL are shorter than in the i2c spec,
    // presumably because actual scl_low and scl_high will be longer due to the
    // above read delay. These values are the values used in the examples in
    // RM0444 section 32.4.11.
    const MIN_SCL_LOW_NS: u32 = 1_250;
    const MIN_SCL_HIGH_NS: u32 = 500;

    // Time from the falling edge of SCL to when we update SDA with the next bit
    // of data.
    const DATA_HOLD_NS: u32 = 375;
    // Time from when we set SDA to when we allow SCL to rise.
    const DATA_SETUP_NS: u32 = 500;

    // The I2C peripheral is clocked by the APB clock.
    let apb_freq = rcc.clocks.apb_clk;
    // Compute the duration of a cycle of SCL sans the read delays.
    let scl_cycle_ns = 1_000_000_000 / i2c_speed.0 - SCL_READ_DELAY_NS;
    // Allocate 5/7ths of the cycle to having SCL high and 2/7ths to low, with
    // each being at least their minimum duration. 5/7ths is chosen based on the
    // minimum values.
    let scl_low_ns = core::cmp::max(scl_cycle_ns * 5 / 7, MIN_SCL_LOW_NS);
    let scl_high_ns = core::cmp::max(scl_cycle_ns - scl_low_ns, MIN_SCL_HIGH_NS);
    let prescaler = (apb_freq.0 / TARGET_CLOCK).saturating_sub(1);
    let presc_ns = 1_000_000_000 / (apb_freq.0 / (prescaler + 1));
    // SCL will be low for presc_ns * (scl_low + 1). When computing scl_low, we
    // add `presc_ns - 1` so that we in effect round up, as it's better for
    // times to be slightly longer than the spec says than slightly shorter.
    let scl_low = ((scl_low_ns + presc_ns - 1) / presc_ns) - 1;
    let scl_high = (scl_high_ns + presc_ns - 1) / presc_ns - 1;
    let scldel = (DATA_SETUP_NS + presc_ns - 1) / presc_ns - 1;
    // RM0444 shows the time for SDADEL as `SDADEL * presc_ns + t(apb_freq)`.
    // i.e. unlike SCLDEL, it doesn't add 1 before multiplying by presc_ns. So
    // we don't subtract 1 here when computing SDADEL. We don't take into
    // consideration the single cycle of the APB clock in this time.
    let sdadel = (DATA_HOLD_NS + presc_ns - 1) / presc_ns;

    // Pack the configuration values as specified in RM0444 section 32.7.5,
    // suitable for putting into the I2C timing register.
    prescaler << 28 | scldel << 20 | sdadel << 16 | scl_high << 8 | scl_low
}

macro_rules! i2c_bus_controller {
    ($i2c:path, $enable_bit:ident, $reset_bit:ident, $interrupt:path) => {
        impl I2cBusController for $i2c {
            fn enable_power(_rcc: &mut Rcc) {
                // safety: We hold an exclusive reference to Rcc, which owns RCC. Unfortunately Rcc doesn't
                // provide the access we need.
                let raw_rcc = unsafe { hal::stm32::Peripherals::steal().RCC };
                // Enable I2C peripheral clock.
                raw_rcc.apbenr1.modify(|_, w| w.$enable_bit().set_bit());
            }

            fn reset_peripheral(_rcc: &mut Rcc) {
                // safety: We hold an exclusive reference to Rcc, which owns RCC. Unfortunately Rcc doesn't
                // provide the access we need.
                let raw_rcc = unsafe { hal::stm32::Peripherals::steal().RCC };
                // Reset the I2C peripheral.
                raw_rcc.apbrstr1.modify(|_, w| w.$reset_bit().set_bit());
                raw_rcc.apbrstr1.modify(|_, w| w.$reset_bit().clear_bit());
            }

            fn interrupt() -> hal::stm32::Interrupt {
                $interrupt
            }
        }
    };
}

i2c_bus_controller!(stm32::I2C1, i2c1en, i2c1rst, stm32::Interrupt::I2C1);
i2c_bus_controller!(stm32::I2C2, i2c2en, i2c2rst, stm32::Interrupt::I2C2);
