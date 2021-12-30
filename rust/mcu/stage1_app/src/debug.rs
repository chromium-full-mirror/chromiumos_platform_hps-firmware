// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use hal::rcc::Rcc;
use i2c_protocol::Buffer;
use i2c_protocol::WriteMemoryEvent;
use log::error;
use log::info;
use mcu_common::memory_banks;
use mcu_common::McuDebugCommand;
use mcu_common::MemBlock;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use mcu_common::DEBUG_COMMAND_START;
use rtt_target::DownChannel;
use rtt_target::UpChannel;

use crate::board::CameraI2c;

pub struct DebugResources {
    pub(crate) fpga_up_channel: UpChannel,
    #[cfg(feature = "image-transfer")]
    pub(crate) image_channel: UpChannel,
    pub(crate) mcu_cmd_response_channel: UpChannel,
    pub(crate) fpga_debug_handler: DebugCommandHandler,
    pub(crate) mcu_debug_handler: DebugCommandHandler,
}

pub struct DebugCommandHandler {
    channel: DownChannel,

    buffer: [u8; DEBUG_BYTES_PER_COMMAND],
    bytes_received: usize,
    pending: Option<McuDebugCommandInfo>,
    mem_block: Option<MemBlock>,
}

pub(crate) struct McuDebugCommandInfo {
    pub(crate) kind: McuDebugCommand,
    pub(crate) arg: u16,
    pub(crate) write_request: Option<WriteMemoryEvent>,
}

impl DebugCommandHandler {
    pub(crate) fn new(channel: DownChannel) -> Self {
        Self {
            channel,
            buffer: [0u8; DEBUG_BYTES_PER_COMMAND],
            bytes_received: 0,
            pending: None,
            mem_block: None,
        }
    }

    pub(crate) fn supply_mem_block(&mut self, mem_block: MemBlock) {
        self.mem_block = Some(mem_block);
    }

    /// Writes the next debug command into `buffer`. Leaves `buffer` unchanged
    /// if there is no next command, or if it is incomplete.
    pub(crate) fn get_debug_command(&mut self, buffer: &mut [u8]) {
        let r = self.channel.read(&mut self.buffer[self.bytes_received..]);
        self.bytes_received += r;
        while self.bytes_received >= 1 && self.buffer[0] != DEBUG_COMMAND_START {
            self.buffer.copy_within(1..DEBUG_BYTES_PER_COMMAND, 0);
            self.bytes_received -= 1;
            self.bytes_received += self.channel.read(&mut self.buffer[self.bytes_received..]);
        }
        if self.bytes_received == DEBUG_BYTES_PER_COMMAND {
            buffer[..3].clone_from_slice(&self.buffer[1..4]);
            self.bytes_received = 0;
        }
    }

    pub(crate) fn get_mcu_debug_command(&mut self) -> Option<McuDebugCommandInfo> {
        // If we're in the process of receiving a write request, then receive
        // bytes into the appropriate buffer.
        if let Some(command) = self.pending.as_mut() {
            // The following upwrap must succeed because we don't put anything
            // into `pending` without it having a write request.
            let write_request = command.write_request.as_mut().unwrap();
            let r = self
                .channel
                .read(&mut write_request.data[self.bytes_received..]);
            self.bytes_received += r;
            if self.bytes_received == write_request.data.len() {
                self.bytes_received = 0;
                return self.pending.take();
            }
            return None;
        }
        // Receive a regular command.
        let mut cmd_buf = [0u8; 3];
        self.get_debug_command(&mut cmd_buf);
        if cmd_buf[0] == 0 {
            return None;
        }
        let kind = match McuDebugCommand::try_from(cmd_buf[0]) {
            Ok(cmd) => cmd,
            Err(_) => {
                error!("Unknown MCU command {}", cmd_buf[0]);
                return None;
            }
        };
        let arg = u16::from_le_bytes([cmd_buf[1], cmd_buf[2]]);
        let mut command = McuDebugCommandInfo {
            kind,
            arg,
            write_request: None,
        };
        if command.kind == McuDebugCommand::WriteSpiFlash {
            if let Some(mem_block) = self.mem_block.take() {
                let mut buffer = Buffer::new(mem_block);
                buffer.resize(buffer.capacity());
                command.write_request = Some(WriteMemoryEvent {
                    // Note, bank is not actually used by the caller, however if
                    // it were, FPGA_BITSTREAM represents the start of the SPI
                    // flash, so would do what we wanted.
                    bank: memory_banks::FPGA_BITSTREAM,
                    address: arg as u32 * 256,
                    data: buffer,
                });
            } else {
                error!("Attempt to write SPI flash when buffer is unavailable");
                return None;
            }
            self.pending = Some(command);
            None
        } else {
            Some(command)
        }
    }
}

pub(crate) fn init_debug_timer(
    tim16: hal::stm32::TIM16,
    rcc: &mut Rcc,
) -> hal::timer::Timer<hal::stm32::TIM16> {
    use hal::prelude::*;

    // Set up a periodic timer. This is only needed for polling our debug
    // channels, so isn't needed in production. 100Hz is chosen in order to
    // maximize the speed of writing to SPI flash from hps-mon. Going faster
    // than 100Hz gives little to no gain in write speed.
    let mut timer = tim16.timer(rcc);
    timer.start(100.hz());
    timer.listen();
    timer
}

pub(crate) fn read_camera_id(camera_i2c: &mut CameraI2c) {
    let mut camera = hm01b0::Camera::new(camera_i2c);
    match camera.get_camera_id() {
        Ok(v) => info!("0x{:04x}", v),
        Err(_) => info!("Failed to read camera ID."),
    }
}

pub(crate) fn read_camera_ae_config(camera_i2c: &mut CameraI2c) {
    let mut camera = hm01b0::Camera::new(camera_i2c);
    match camera.get_ae_target_mean() {
        Ok(v) => info!("AE Target:\t{} (0x{:02x})", v, v),
        Err(_) => info!("AE Target:\tFailed"),
    }
    match camera.get_ae_mean() {
        Ok(v) => info!("AE Mean:\t{} (0x{:02x})", v, v),
        Err(_) => info!("AE Mean:\tFailed"),
    }
    match camera.get_ae_min_mean() {
        Ok(v) => info!("AE Min Mean:\t{} (0x{:02x})", v, v),
        Err(_) => info!("AE Min Mean:\tFailed"),
    }
    match camera.get_ae_converge_in() {
        Ok(v) => info!("Conv In:\t{} (0x{:02x})", v, v),
        Err(_) => info!("Conv In:\tFailed"),
    }
    match camera.get_ae_converge_out() {
        Ok(v) => info!("Conv Out:\t{} (0x{:02x})", v, v),
        Err(_) => info!("Conv Out:\tFailed"),
    }
}

pub(crate) fn read_camera_gains(camera_i2c: &mut CameraI2c) {
    let mut camera = hm01b0::Camera::new(camera_i2c);
    match camera.get_analog_gain() {
        Ok(v) => info!("Analog Gain:\t{} (0x{:02x})", v, v),
        Err(_) => info!("Analog Gain:\tFailed"),
    }
    match camera.get_digital_gain() {
        Ok(v) => info!("Digital Gain: {} (0x{:04x})", v, v),
        Err(_) => info!("Digital Gain: Failed"),
    }
}

pub(crate) fn read_camera_integration(camera_i2c: &mut CameraI2c) {
    let mut camera = hm01b0::Camera::new(camera_i2c);
    match camera.get_integration() {
        Ok(v) => info!("Integration:\t{} (0x{:04x})", v, v),
        Err(_) => info!("Integration:\tFailed"),
    }
}
