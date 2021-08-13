// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![no_std]

use embedded_hal::blocking::i2c;

const CAMERA_I2C_ID: u8 = 0x24;

/// Interface to configure the HM01B0 camera.
pub struct Camera<'a, I: i2c::Write + i2c::WriteRead> {
    i2c: &'a mut I,
}

pub enum Error {
    I2cError,
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

    pub fn reset(&mut self) -> Result<()> {
        self.write_reg(0x0103, 0)
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
