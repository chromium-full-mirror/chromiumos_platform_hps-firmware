// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod console;
mod debug_commands;
mod image_channel;
mod openocd;
mod text_channel;
mod write_spi_flash;

#[cfg(feature = "direct")]
mod direct;

use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use colored::*;
use console::Console;
use console::ConsoleController;
use image_channel::image_type_from_str;
use image_channel::ImageType;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use text_channel::TextChannel;

use crate::debug_commands::Command;
use crate::debug_commands::LocalOperation;

const SOC_ROM_OFFSET: usize = 2 * 1024 * 1024;

struct Channel {
    up_channel: Box<dyn DataSource>,
    handler: Box<dyn ChannelHandler>,
}

impl Channel {
    fn new(mut up_channel: Box<dyn DataSource>, handler: Box<dyn ChannelHandler>) -> Result<Self> {
        // Make our channels block on the device side so that we don't miss any
        // messages. This also serves to let the device side know that we're
        // here, otherwise it will send the FPGA output to channel 0 for
        // compatibility with probe-run.
        up_channel.set_blocking_mode()?;
        let mut channel = Self {
            up_channel,
            handler,
        };
        // Clear any existing data in the channel, otherwise stale data can
        // appear, which can be confusing.
        channel.clear()?;
        Ok(channel)
    }

    /// Clear any existing data already on this channel.
    fn clear(&mut self) -> Result<()> {
        let mut buffer = vec![0u8; 1024];
        while self.up_channel.read(&mut buffer)? != 0 {}
        Ok(())
    }
}

trait ChannelHandler {
    /// Handles data received from the channel.
    fn handle(&mut self, bytes: &[u8]) -> Result<()>;
}

/// A source from which we can read data.
trait DataSource {
    /// Reads bytes into `bytes`, returning how many bytes were read.
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize>;

    /// Sets whether reads should block. This is only implemented for openocd.
    fn set_read_blocking(&mut self, _blocking: bool) -> Result<()> {
        // Not implemented. We'll fall back to polling.
        Ok(())
    }

    /// Sets this data source so that writes to the on-device buffer will block
    /// if it fills up.
    fn set_blocking_mode(&mut self) -> Result<()>;
}

/// A sink through which we can write data to the device.
trait DataSink {
    /// Writes `bytes` to the device.
    fn write(&mut self, bytes: &[u8]) -> Result<()>;
}

trait Device {
    /// Reset the device.
    fn reset(&mut self) -> Result<()>;

    fn attach_rtt(&mut self) -> Result<RttChannels>;
}

struct RttChannels {
    from_device: Vec<Box<dyn DataSource>>,
    to_device: Vec<Box<dyn DataSink>>,
}

#[derive(FromArgs)]
/// Monitors the HPS for debug output via SWD.
struct Flags {
    #[cfg(feature = "direct")]
    #[argh(option)]
    /// CPU model
    cpu: Option<String>,

    #[argh(switch)]
    /// reset MCU on start
    reset: bool,

    #[argh(option)]
    /// directory into which images should be written
    image_out: Option<PathBuf>,

    #[argh(option)]
    /// filename from which gateware should be read when write_gateware command
    /// is run
    gateware: Option<PathBuf>,

    #[argh(option)]
    /// filename from which SOC ROM should be read when write_soc_rom command is
    /// run
    soc_rom: Option<PathBuf>,

    #[argh(
        option,
        default = "ImageType::Grayscale",
        from_str_fn(image_type_from_str)
    )]
    /// image type that we expect from the camera
    image_type: ImageType,

    #[cfg(feature = "direct")]
    #[argh(option, default = "0")]
    /// which probe number to use, starting from 0.
    probe_num: usize,

    #[argh(option)]
    /// if specified, connects to openocd on this port instead of using an
    /// st-link
    openocd_port: Option<u16>,

    #[argh(option, default = "9000")]
    /// base port to use for RTT channels. This port and the subsequent
    /// 4 ports must not already be in use.
    base_port: u16,

    #[argh(option, default = "String::new()")]
    /// commands to run on startup
    cmd: String,
}

fn main() -> Result<()> {
    let flags: Flags = argh::from_env();
    let mut device;
    if let Some(openocd_port) = flags.openocd_port {
        device = openocd::OpenOcdDevice::new(openocd_port, flags.base_port)?;
    } else {
        #[cfg(feature = "direct")]
        {
            device = direct::DirectDevice::new(&flags)?;
        }
        #[cfg(not(feature = "direct"))]
        {
            bail!("probe-rs feature is not enabled and you didn't specify --openocd-port");
        }
    }

    if flags.reset {
        device.reset()?;
    }

    let console_controller = ConsoleController::new();
    // Probably not strictly necessary, but we have an explicit scope to make
    // sure that `console` goes out of scope before we call `shutdown` on
    // `console_controller`.
    {
        let console = console_controller.console();
        let mut rtt = device.attach_rtt()?;
        let mut cmd_response_channel = None;
        if rtt.from_device.len() >= 4 {
            cmd_response_channel = rtt.from_device.drain(3..=3).next().and_then(|mut channel| {
                if channel.set_read_blocking(true).is_ok() {
                    Some(channel)
                } else {
                    None
                }
            });
        }

        let channels = create_channels(rtt.from_device, &flags, console.new_line_sink())?;
        let mut to_device = rtt.to_device.drain(..);
        run_loop(
            channels,
            to_device.next(),
            to_device.next(),
            console,
            cmd_response_channel,
            &flags,
            &mut *device,
        )?;
    }
    console_controller.shutdown();
    Ok(())
}

/// Create channels. The channels here must match the channel definitions in
/// stm32g0_application/src/main.rs. e.g. up-channel 0 is the MCU's debug
/// channel, 1 is the FPGA's debug channel.
fn create_channels(
    mut from_device: Vec<Box<dyn DataSource>>,
    flags: &Flags,
    line_sink: mpsc::Sender<String>,
) -> Result<Vec<Channel>> {
    let mut raw_channels = from_device.drain(..);
    let mut channels = Vec::new();
    if let Some(up_channel) = raw_channels.next() {
        channels.push(Channel::new(
            up_channel,
            Box::new(TextChannel::new("MCU> ".yellow(), line_sink.clone())),
        )?);
    }
    if let Some(up_channel) = raw_channels.next() {
        channels.push(Channel::new(
            up_channel,
            Box::new(TextChannel::new("FPGA> ".green(), line_sink.clone())),
        )?);
    }
    if let (Some(image_out), Some(up_channel)) = (&flags.image_out, raw_channels.next()) {
        channels.push(Channel::new(
            up_channel,
            Box::new(image_channel::ImageChannel::new(
                image_out,
                flags.image_type,
                line_sink,
            )?),
        )?);
    }
    Ok(channels)
}

fn run_loop(
    mut channels: Vec<Channel>,
    mut mcu_command_channel: Option<Box<dyn DataSink>>,
    mut fpga_command_channel: Option<Box<dyn DataSink>>,
    console: Console,
    mut cmd_response_channel: Option<Box<dyn DataSource>>,
    flags: &Flags,
    device: &mut dyn Device,
) -> Result<()> {
    let mut backoff_ms = 0;
    let mut buffer = vec![0; 1024];
    let command_source = debug_commands::create_command_source(console, &flags.cmd)?;
    loop {
        let mut got_data = false;
        for channel in &mut channels {
            let count = channel.up_channel.read(&mut buffer)?;
            if count > 0 {
                got_data = true;
                if let Err(e) = channel.handler.handle(&buffer[..count]) {
                    eprintln!("Channel receive error: {}", e);
                }
            }
        }
        if let Ok(command_kind) = command_source.try_recv() {
            match command_kind {
                Command::FpgaCommand(c) => {
                    send_command_to_device(&c, &mut &mut fpga_command_channel)?
                }
                Command::McuCommand(c) => {
                    send_command_to_device(&c, &mut &mut mcu_command_channel)?
                }
                Command::LocalOperation(LocalOperation::PrintHelp) => {
                    debug_commands::print_help_text();
                }
                Command::LocalOperation(LocalOperation::Exit) => {
                    return Ok(());
                }
                Command::LocalOperation(LocalOperation::ResetMcu) => {
                    device.reset()?;
                }
                Command::LocalOperation(LocalOperation::WriteGateware) => {
                    if let Some(file) = &flags.gateware {
                        if let Err(error) = write_spi_flash::write_spi_flash(
                            &mut mcu_command_channel,
                            &mut cmd_response_channel,
                            0,
                            file,
                        ) {
                            eprintln!("{}", error);
                        }
                    } else {
                        eprintln!("--gateware must be specified on startup");
                    }
                }
                Command::LocalOperation(LocalOperation::WriteSocRom) => {
                    if let Some(file) = &flags.soc_rom {
                        if let Err(error) = write_spi_flash::write_spi_flash(
                            &mut mcu_command_channel,
                            &mut cmd_response_channel,
                            SOC_ROM_OFFSET,
                            file,
                        ) {
                            eprintln!("{}", error);
                        }
                    } else {
                        eprintln!("--soc-rom must be specified on startup");
                    }
                }
            };
        }
        // If we don't get any data from the device, we sleep, increasing how
        // long we sleep each time until we hit a maximum.
        if got_data {
            backoff_ms = 0;
        } else {
            if backoff_ms < 100 {
                backoff_ms += 10;
            }
            std::thread::sleep(Duration::from_millis(backoff_ms));
        }
    }
}

fn send_command_to_device(
    command: &debug_commands::DebugCommand,
    command_channel: &mut Option<Box<dyn DataSink>>,
) -> Result<()> {
    if let Some(command_channel) = command_channel.as_mut() {
        command_channel.write(&command.to_bytes())?;
    } else {
        eprintln!("No command channel available, command not sent.");
    }
    Ok(())
}
