// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::convert::TryFrom;
use crc::Crc;
use hal::prelude::OutputPin;
use hal::rcc::Rcc;
use hal::timer::stopwatch::Stopwatch;
use i2c_protocol::WriteMemoryEvent;
use log::error;
use log::info;
use mcu_common::memory_banks;
use mcu_common::Buffer;
use mcu_common::McuDebugCommand;
use mcu_common::MemBlock;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use mcu_common::DEBUG_COMMAND_START;
use mcu_common::SPI_BLOCK_SIZE;
use rtic::Mutex;
use rtt_target::rtt_init;
use rtt_target::ChannelMode;
use rtt_target::DownChannel;
use rtt_target::UpChannel;
use spi_memory::series25::Flash;

use crate::board::CameraI2c;
use crate::fpga_device::get_spi_flash;
use crate::spi_flash::SpiFlash;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);

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

impl DebugResources {
    /// # Safety
    /// Must only be called once. Must be called while interrupts are disabled.
    pub unsafe fn init() -> DebugResources {
        let channels = rtt_init!(
            up: {
                0: {
                    size: 256
                    mode: NoBlockSkip
                    name: "MCU"
                }
                1: {
                    size: 256
                    mode: NoBlockSkip
                    name: "FPGA"
                }
                2: {
                    size: 1024
                    mode: BlockIfFull
                    name: "Images"
                }
                3: {
                    size: 128
                    mode: NoBlockSkip
                    name: "McuCmdResponses"
                }
            }
            down: {
                0: {
                    // This must be at least large enough to fit 256 *
                    // MAX_OUTSTANDING_COMMANDS (as defined in hps-mon).
                    size: 8192
                    mode: NoBlockSkip
                    name: "McuCommands"
                }
                1: {
                    size: 16
                    mode: NoBlockSkip
                    name: "FpgaCommands"
                }
            }
        );
        rtt_target::set_print_channel(channels.up.0);
        debug_logger::initialize_logging();

        DebugResources {
            fpga_up_channel: channels.up.1,
            #[cfg(feature = "image-transfer")]
            image_channel: channels.up.2,
            mcu_cmd_response_channel: channels.up.3,
            mcu_debug_handler: DebugCommandHandler::new(channels.down.0),
            fpga_debug_handler: DebugCommandHandler::new(channels.down.1),
        }
    }
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

pub(crate) fn set_group_param_hold(camera_i2c: &mut CameraI2c) {
    let mut camera = hm01b0::Camera::new(camera_i2c);
    if let Err(_) = camera.set_group_param_hold() {
        info!("Failed to set group param hold");
    }
}

#[cfg(feature = "image-transfer")]
pub(crate) fn transfer_image(
    buffer: &MemBlock,
    shared: &mut crate::app::handle_spi_packet::SharedResources,
) -> bool {
    let i2c_image_xfer_enabled = shared.state.lock(|s| s.i2c_image_transfer_enabled());
    let mut success = true;
    if i2c_image_xfer_enabled {
        shared.host_interface.lock(|host_interface| {
            success = host_interface.queue_image_data_for_i2c(&buffer[1..]);
        });
    } else {
        shared.debug_resources.image_channel.write(&buffer[1..]);
    }
    success
}

pub(crate) fn handle_mcu_debug_command(
    command: McuDebugCommandInfo,
    resources: &mut crate::app::debug_timer_tick::SharedResources,
) {
    match command.kind {
        McuDebugCommand::ReadSpiFlash => debug_read(
            get_spi_flash(resources.fpga_device).ok(),
            command.arg as u32,
        ),
        McuDebugCommand::WriteSpiFlash => {
            if let Some(write_request) = command.write_request {
                // For now, we ignore returned errors here, since the write
                // function will have printed the error message in more detail
                // already.
                let _ = crate::spi_flash::write(
                    get_spi_flash(resources.fpga_device).ok(),
                    write_request.address,
                    &*write_request.data,
                );
                resources
                    .debug_resources
                    .mcu_debug_handler
                    .supply_mem_block(write_request.data.release());
                resources
                    .debug_resources
                    .mcu_cmd_response_channel
                    .write(&[0, 0, 0, 0]);
            }
        }
        McuDebugCommand::EraseSpiFlash => {
            // For now, we ignore returned errors here, since the write
            // function will have printed the error message in more detail
            // already.
            let _ = crate::spi_flash::erase(get_spi_flash(resources.fpga_device).ok());
        }
        McuDebugCommand::HashSpiFlash => time_hash(
            get_spi_flash(resources.fpga_device).ok(),
            command.arg as u32,
            resources.stopwatch,
        ),
        McuDebugCommand::SpiFlashCrc => {
            resources
                .debug_resources
                .mcu_cmd_response_channel
                .write(&crc_sector(
                    get_spi_flash(resources.fpga_device).ok(),
                    command.arg,
                ));
        }
        McuDebugCommand::SpiFlashReadSpeed => {
            test_read_speed(
                get_spi_flash(resources.fpga_device).ok(),
                resources.stopwatch,
            );
        }
        McuDebugCommand::SpiFlashWriteSpeed => {
            test_write_speed(
                get_spi_flash(resources.fpga_device).ok(),
                resources.stopwatch,
            );
        }
        McuDebugCommand::FpgaPowerOff => {
            resources.fpga_device.power_gate.set_high().unwrap();
            info!("FPGA and SPI flash powered off");
        }
        McuDebugCommand::FpgaPowerOn => {
            resources.fpga_device.power_gate.set_low().unwrap();
            info!("FPGA and SPI flash powered on");
        }
        McuDebugCommand::ResetFpga => {
            resources.fpga_device.reset_fpga();
        }
        McuDebugCommand::ReadCameraRegister => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match c.read_reg(command.arg) {
                Ok(v) => info!("Register 0x{:04x} = 0x{:02x}", command.arg, v),
                Err(_) => info!("Failed to read 0x{:04x}", command.arg),
            }
        }
        McuDebugCommand::ReadCameraId => read_camera_id(resources.camera_i2c),
        McuDebugCommand::ReadCameraConfig => {
            read_camera_ae_config(resources.camera_i2c);
            read_camera_gains(resources.camera_i2c);
            read_camera_integration(resources.camera_i2c);
        }
        McuDebugCommand::SetCameraBlcTarget => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match command.arg {
                0..=255 => {
                    let config = hm01b0::BlackLevelConfig {
                        blc_enabled: true,
                        blc_target: command.arg as u8,
                        blc2_enabled: true,
                        blc2_target: command.arg as u8,
                    };
                    match c.set_blc_config(config) {
                        Ok(_) => info!("Set black level config target"),
                        Err(_) => info!("Failed to set black level config target"),
                    }
                }
                _ => info!("Black level out of range"),
            }
        }
        McuDebugCommand::SetCameraDigitalGain => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match c.set_digital_gain(command.arg) {
                Ok(_) => read_camera_gains(resources.camera_i2c),
                Err(_) => info!("Failed to write digital gain"),
            }
            set_group_param_hold(resources.camera_i2c)
        }
        McuDebugCommand::SetCameraAnalogGain => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match c.set_analog_gain(command.arg) {
                Ok(_) => read_camera_gains(resources.camera_i2c),
                Err(_) => info!("Failed to write analog gain"),
            }
            set_group_param_hold(resources.camera_i2c)
        }
        McuDebugCommand::SetCameraIntegration => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            info!("Writing {}", command.arg);
            match c.set_integration(command.arg) {
                Ok(_) => read_camera_integration(resources.camera_i2c),
                Err(_) => info!("Failed to write integration"),
            }
            set_group_param_hold(resources.camera_i2c)
        }
        McuDebugCommand::SetCameraAeTarget => {
            let mut c = hm01b0::Camera::new(resources.camera_i2c);
            match c.set_ae_target(command.arg) {
                Ok(_) => read_camera_ae_config(resources.camera_i2c),
                Err(_) => info!("Failed to write AE target"),
            }
        }
        McuDebugCommand::TryStartFpga => {
            resources.host_interface.lock(|h| h.command_triggered());
            let _ = crate::app::try_start_fpga::spawn();
        }
        McuDebugCommand::SetBlockingMode => {
            let mode = if command.arg == 1 {
                ChannelMode::BlockIfFull
            } else {
                ChannelMode::NoBlockSkip
            };
            resources.debug_resources.fpga_up_channel.set_mode(mode);
            // Ideally we'd like to set the mode of the MCU up channel as well,
            // but we passed ownership of it to rtt_target::set_print_channel.
        }
        McuDebugCommand::Ping => info!("Pong!"),
    }
}

pub(crate) fn debug_read(
    spi_flash: Option<&mut Flash<crate::board::FlashSpiControl, crate::board::FlashSpiCs>>,
    address: u32,
) {
    use spi_memory::Read;

    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    let mut buffer = [0u8; 16];
    if spi_flash.read(address, &mut buffer).is_err() {
        error!("Failed to read SPI flash");
        return;
    }
    info!("SPI flash bytes");
    for v in &buffer {
        info!("{}, ", v);
    }
}

pub(crate) fn crc_sector(
    spi_flash: Option<&mut Flash<crate::board::FlashSpiControl, crate::board::FlashSpiCs>>,
    block: u16,
) -> [u8; 4] {
    use crate::spi_flash::PAGE_SIZE;
    use spi_memory::Read;

    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return [0, 0, 0, 0];
        }
    };
    let mut digest = CRC.digest();
    let mut buf = [0x00; PAGE_SIZE as usize];
    const PAGES_PER_BLOCK: u32 = SPI_BLOCK_SIZE / PAGE_SIZE;
    for page_offset in 0..PAGES_PER_BLOCK {
        let _ = spi_flash.read(
            (block as u32 * PAGES_PER_BLOCK + page_offset) * PAGE_SIZE,
            &mut buf,
        );
        digest.update(&buf);
    }
    digest.finalize().to_le_bytes()
}

fn test_read_speed(
    spi_flash: Option<&mut Flash<crate::board::FlashSpiControl, crate::board::FlashSpiCs>>,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    use crate::spi_flash::PAGE_SIZE;
    use spi_memory::Read;

    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    info!("Reading...");
    const READ_SIZE: u32 = 1024 * 1024;
    let mut buf = [0u8; PAGE_SIZE as usize];
    let elapsed_us = stopwatch.trace(|| {
        for page in 0..(READ_SIZE / PAGE_SIZE) {
            let _ = spi_flash.read(page * PAGE_SIZE, &mut buf);
        }
    });
    info!("Reading 1MB took {}us", elapsed_us.0);
}

fn test_write_speed(
    spi_flash: Option<&mut Flash<crate::board::FlashSpiControl, crate::board::FlashSpiCs>>,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    use crate::spi_flash::PAGE_SIZE;
    use spi_memory::BlockDevice;

    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    info!("Writing...");
    const READ_SIZE: u32 = 1024 * 1024;
    let mut buf = [42u8; PAGE_SIZE as usize];
    let mut erase_time = 0;
    let mut write_time = 0;
    for page in 0..(READ_SIZE / PAGE_SIZE) {
        let address = 9 * 1024 * 1024 + page * PAGE_SIZE;
        if address % SPI_BLOCK_SIZE == 0 {
            erase_time += stopwatch
                .trace(|| {
                    // safety: We hold a mutable reference to Flash, which owns
                    // the same peripheral.
                    let mut spi = unsafe { SpiFlash::steal() };
                    let _ = spi.erase_block(address);
                })
                .0;
        }
        write_time += stopwatch
            .trace(|| {
                let _ = spi_flash.write_bytes(address, &mut buf);
            })
            .0;
    }
    info!(
        "Erasing 1MB took {}ms. Writing 1MB took {}ms",
        erase_time / 1000,
        write_time / 1000
    );
}

pub(crate) fn time_hash(
    spi_flash: Option<&mut Flash<crate::board::FlashSpiControl, crate::board::FlashSpiCs>>,
    num_pages: u32,
    stopwatch: &mut Stopwatch<hal::stm32::TIM2>,
) {
    let spi_flash = match spi_flash {
        Some(x) => x,
        None => {
            error!("SPI flash not available");
            return;
        }
    };
    let elapsed_us = stopwatch.trace(|| {
        let hash = crate::spi_flash::hash_internal(spi_flash, 0, num_pages);
        info!("Hash of first {} pages was: {:?}", num_pages, hash);
    });
    info!("Hashing took {}us", elapsed_us.0);
}
