// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

mod init_script;

use embedded_hal::blocking::i2c;

const CAMERA_I2C_ID: u8 = 0x24;

/// Interface to configure the HM01B0 camera.
pub struct Camera<'a, I: i2c::Write + i2c::WriteRead> {
    i2c: &'a mut I,
}

#[derive(Debug)]
pub enum Error {
    I2cError,
    InvalidValues,
    PartialReset,
}

#[derive(Debug, Copy, Clone)]
pub enum TestPattern {
    Walking1s,
    ColorBar,
}

pub enum Mode {
    Standby,
    Streaming,
    StreamingNFrames(u8),
    Trigger,
}

pub struct ShiftConfig {
    pub pixel_shift_enabled: bool,
    pub hsync_shift_enabled: bool,
    pub vsync_shift_enabled: bool,
}

pub struct BlackLevelConfig {
    pub blc_enabled: bool,
    pub blc_target: u8,
    pub blc2_enabled: bool,
    pub blc2_target: u8,
}

// Attributes related to the camera's exposure
#[derive(Debug, PartialEq)]
pub struct Exposure {
    // Valid values are from 2 to frame length
    pub integration: u16,
    // Valid values are from 0x0000 to 0x3fc0 (2 LSBs are '0')
    pub digital_gain: u16,
    // Valid values are 0x00, 0x10, 0x20, 0x30
    pub analog_gain: u16,
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

impl<'a, I: i2c::Write + i2c::WriteRead> Camera<'a, I> {
    pub fn new(i2c: &'a mut I) -> Self {
        Self { i2c }
    }

    pub fn write_reg(&mut self, register: u16, value: u8) -> Result<()> {
        let mut buf = [0u8; 3];
        buf[..2].copy_from_slice(&register.to_be_bytes());
        buf[2] = value;
        self.i2c
            .write(CAMERA_I2C_ID, &buf)
            .map_err(|_| Error::I2cError)
    }

    pub fn read_reg(&mut self, register: u16) -> Result<u8> {
        let mut result = [0u8; 1];
        self.i2c
            .write_read(CAMERA_I2C_ID, &register.to_be_bytes(), &mut result)
            .map_err(|_| Error::I2cError)?;
        Ok(result[0])
    }

    pub fn get_camera_id(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes([
            self.read_reg(0x0000)?,
            self.read_reg(0x0001)?,
        ]))
    }

    pub fn get_ae_target_mean(&mut self) -> Result<u8> {
        self.read_reg(0x2101)
    }

    pub fn get_ae_mean(&mut self) -> Result<u8> {
        self.read_reg(0x2020)
    }

    pub fn get_ae_min_mean(&mut self) -> Result<u8> {
        self.read_reg(0x2102)
    }

    pub fn get_ae_converge_in(&mut self) -> Result<u8> {
        self.read_reg(0x2103)
    }

    pub fn get_ae_converge_out(&mut self) -> Result<u8> {
        self.read_reg(0x2104)
    }

    pub fn get_digital_gain(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes([
            self.read_reg(0x020E)?,
            self.read_reg(0x020F)?,
        ]))
    }

    pub fn get_analog_gain(&mut self) -> Result<u8> {
        self.read_reg(0x0205)
    }

    pub fn get_integration(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes([
            self.read_reg(0x0202)?,
            self.read_reg(0x0203)?,
        ]))
    }

    pub fn set_digital_gain(&mut self, value: u16) -> Result<()> {
        self.write_reg(0x020E, high_byte(value))?;
        self.write_reg(0x020F, low_byte(value))
    }

    pub fn set_analog_gain(&mut self, value: u16) -> Result<()> {
        match value {
            0x00 | 0x10 | 0x20 | 0x30 => self.write_reg(0x0205, low_byte(value)),
            _ => Err(Error::InvalidValues),
        }
    }

    /// Checks that the first 0x3*** series register that the init script wrote
    /// is still correct. If we don't have a long enough delay between
    /// performing a software reset and running the init script, a partial reset
    /// can sometimes occur. When a partial reset occurs, all 0x3*** series and
    /// 0x0*** series registers get reverted to their reset values.
    pub fn check_for_partial_reset(&mut self) -> Result<()> {
        if self.read_reg(0x3044)? == 0x0A {
            Ok(())
        } else {
            Err(Error::PartialReset)
        }
    }

    pub fn set_integration(&mut self, value: u16) -> Result<()> {
        // Minimum integration allowed is 2
        let val = u16::max(2, value);
        self.write_reg(0x0202, high_byte(val))?;
        self.write_reg(0x0203, low_byte(val))
    }

    pub fn set_exposure(&mut self, exposure: &Exposure) -> Result<()> {
        self.set_integration(exposure.integration)?;
        self.set_digital_gain(exposure.digital_gain)?;
        self.set_analog_gain(exposure.analog_gain)?;
        self.set_group_param_hold()
    }

    pub fn set_ae_target(&mut self, value: u16) -> Result<()> {
        match value {
            0 => self.write_reg(0x2100, 0),
            1..=255 => {
                self.write_reg(0x2100, 1)?;
                self.write_reg(0x2101, low_byte(value))
            }
            _ => Err(Error::InvalidValues),
        }
    }

    pub fn set_ae_max_integration(&mut self, value: u16) -> Result<()> {
        self.write_reg(0x2105, high_byte(value))?;
        self.write_reg(0x2106, low_byte(value))
    }

    pub fn set_ae_min_integration(&mut self, value: u8) -> Result<()> {
        self.write_reg(0x2107, value)
    }

    pub fn reset(&mut self) -> Result<()> {
        self.write_reg(0x0103, 0)
    }

    /// Writing the group param hold register causes exposure settings to be
    /// applied in frame N+2
    pub fn set_group_param_hold(&mut self) -> Result<()> {
        self.write_reg(0x0104, 1)
    }

    pub fn set_test_pattern(&mut self, pattern: Option<TestPattern>) -> Result<()> {
        self.write_reg(
            0x0601,
            match pattern {
                None => 0,
                Some(TestPattern::Walking1s) => 0x11,
                Some(TestPattern::ColorBar) => 0x01,
            },
        )
    }

    pub fn set_qvga_enable(&mut self, enable: bool) -> Result<()> {
        self.write_reg(0x3010, if enable { 0x01 } else { 0x00 })
    }

    /// Wait until the camera returns to standby mode. Should only be used in
    /// conjunction with Mode::StreamingNFrames.
    pub fn wait_standby(&mut self) -> Result<()> {
        while self.read_reg(0x0100)? != 0 {}
        Ok(())
    }

    pub fn set_mode(&mut self, mode: Mode) -> Result<()> {
        let mode_value = match mode {
            Mode::Standby => 0x00,
            Mode::Streaming => 0x01,
            Mode::StreamingNFrames(frame_count) => {
                self.write_reg(0x3020, frame_count)?;
                0x03
            }
            Mode::Trigger => 0x05,
        };
        self.write_reg(0x0100, mode_value)
    }

    /// Sets the line length in pixel clock cycles.
    pub fn set_line_length(&mut self, length: u16) -> Result<()> {
        self.write_reg(0x0342, high_byte(length))?;
        self.write_reg(0x0343, low_byte(length))
    }

    /// Sets the frame length in lines.
    pub fn set_frame_length(&mut self, length: u16) -> Result<()> {
        self.write_reg(0x0340, high_byte(length))?;
        self.write_reg(0x0341, low_byte(length))
    }

    pub fn set_advance_vsync(&mut self, num_lines: u8) -> Result<()> {
        self.write_reg(0x3022, num_lines)
    }

    pub fn set_shift_config(&mut self, config: ShiftConfig) -> Result<()> {
        self.write_reg(
            0x1012,
            bit(0, config.vsync_shift_enabled)
                | bit(1, config.hsync_shift_enabled)
                | bit(2, config.pixel_shift_enabled),
        )
    }

    // Configures the camera with recommended initial configuration provided
    // by the manufacturer (HIMAX).
    pub fn set_analog_default_config(&mut self) -> Result<()> {
        self.write_reg(0x1003, 0x08)?; // BLC target :8 at 8 bit mode.
        self.write_reg(0x1007, 0x08)?; // BLI target :8 at 8 bit mode.
        self.write_reg(0x3044, 0x0A)?; // Increase CDS time for settling.
        self.write_reg(0x3045, 0x00)?; // Make symetric for cds_tg and rst_tg.
        self.write_reg(0x3047, 0x0A)?; // Increase CDS time for settling.
        self.write_reg(0x3050, 0xC0)?; // Make negative offset up to 4x.
        self.write_reg(0x3051, 0x42)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x3052, 0x50)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x3053, 0x00)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x3054, 0x03)?; // Tuning sf sig clamping as lowest.
        self.write_reg(0x3055, 0xF7)?; // Tuning dsun.
        self.write_reg(0x3056, 0xF8)?; // Increase adc nonoverlap clk.
        self.write_reg(0x3057, 0x29)?; // Increase adc pwr for missing code.
        self.write_reg(0x3058, 0x1F)?; // Turn on dsun.
        self.write_reg(0x3059, 0x1E)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x3064, 0x00)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x3065, 0x04) // Pad pull 0.
    }

    // Sets the camera auto-exposure (AE).
    pub fn set_ae(&mut self, ae_enabled: bool) -> Result<()> {
        self.write_reg(0x2100, bit(0, ae_enabled))
    }

    // Enables the camera auto-exposure (AE).
    // Setting the camera auto-exposure comes with configuration of multiple
    // reserved registers, which are a part of i initial configuration
    // provided by the manufacturer (HIMAX).
    pub fn set_ae_default_config(&mut self) -> Result<()> {
        self.write_reg(0x2100, 0x01)?; // AE enabled.
        self.write_reg(0x2104, 0x07)?; // Converge out threshold.
        self.write_reg(0x2105, 0x02)?; // Maximum INTG HB.
        self.write_reg(0x2106, 0x14)?; // Maximum INTG LB.
        self.write_reg(0x2108, 0x03)?; // Maximum analog gain in full frame mode.
        self.write_reg(0x2109, 0x03)?; // Maximum analog gain in BIN2 mode.
        self.write_reg(0x210B, 0x80)?; // Maximum digital gain.
        self.write_reg(0x210F, 0x00)?; // Flicker step 60Hz HB.
        self.write_reg(0x2110, 0x85)?; // Flicker step 60Hz LB.
        self.write_reg(0x2111, 0x00)?; // Flicker step 50Hz HB.
        self.write_reg(0x2112, 0xA0) // Flicker step 50Hz LB.
    }

    // Enables the Black Level Correction (BLC) and Black Level Indicator (BLC2).
    // Setting BLC and BLC2 comes with configuration of multiple reserved
    // registers, which are a part of initial configuration provided by
    // the manufacturer (HIMAX).
    pub fn set_blc_config(&mut self, config: BlackLevelConfig) -> Result<()> {
        // BLC configuration.
        self.write_reg(0x1000, bit(0, config.blc_enabled))?;
        self.write_reg(0x1001, 0x40)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x1002, 0x32)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x1003, config.blc_target)?;
        // BLC2 configuration.
        self.write_reg(0x1006, bit(0, config.blc2_enabled))?;
        self.write_reg(0x1007, config.blc2_target)?;
        self.write_reg(0x1008, 0x00)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x1009, 0xA0)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x100A, 0x60)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x100B, 0x90)?; // Reserved. Initial HIMAX config.
        self.write_reg(0x100C, 0x40) // Reserved. Initial HIMAX config.
    }

    // Configures the camera binning with the recommended intial configuration
    // provided by the manufacturer (HIMAX).
    pub fn set_binning_default_config(&mut self) -> Result<()> {
        self.write_reg(0x0350, 0x7F)?; // Reserved. Digital gain control.
        self.write_reg(0x0383, 0x00)?; // Full X readout.
        self.write_reg(0x0387, 0x01)?; // Full Y readout.
        self.write_reg(0x0390, 0x00) // Horizontal binning.
    }

    pub fn run_init_script<Delay>(&mut self, delay: &mut Delay) -> Result<()>
    where
        Delay: embedded_hal::blocking::delay::DelayMs<u32>,
    {
        self.reset()?;
        // We need to wait a bit after we reset the camera, otherwise we can
        // sometimes have the camera partially reset while we initialize it.
        // Experimentally, 15ms still results in some partial resets, while 18ms
        // appears to be fine. We give a few extra ms to be sure.
        delay.delay_ms(25);
        for (register, value) in init_script::INIT_SCRIPT {
            self.write_reg(*register, *value)?;
        }
        self.check_for_partial_reset()
    }
}

fn bit(bit_number: u8, value: bool) -> u8 {
    if value {
        1 << bit_number
    } else {
        0
    }
}

fn low_byte(value: u16) -> u8 {
    (value & 0xff) as u8
}

fn high_byte(value: u16) -> u8 {
    (value >> 8) as u8
}
