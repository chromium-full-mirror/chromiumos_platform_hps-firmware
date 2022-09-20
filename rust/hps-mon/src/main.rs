// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod debug_commands;
mod image_channel;
mod openocd;
mod text_channel;
mod write_spi_flash;

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use colored::*;
use debug_commands::ConsoleOutput;
use host_dev_common::images::image_type_from_str;
use host_dev_common::images::ImageType;
use hps_interface::Hps;
use hps_interface::HpsAdapter;
use hps_interface::InterruptLine;
use hps_interface::NoopInterruptLine;
use interrupt_proxy::FtdiInterruptListener;
use mcu_common::Status;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use text_channel::TextChannel;

use crate::debug_commands::Command;
use crate::debug_commands::LocalCommand;
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

    fn write_program(&mut self, filename: &Path) -> Result<()>;
}

struct RttChannels {
    from_device: Vec<Box<dyn DataSource>>,
    to_device: Vec<Box<dyn DataSink>>,
}

/// Which mechanism are we using to observe the interrupt line?
#[derive(clap::ArgEnum, Clone, PartialEq, Debug)]
enum InterruptInterface {
    None,
    AvrFtdiProxy,
}

#[derive(Parser)]
/// Monitors the HPS for debug output via SWD.
struct Flags {
    #[clap(long)]
    /// reset MCU on start
    reset: bool,

    #[clap(long)]
    /// directory into which images should be written
    image_out: Option<PathBuf>,

    #[clap(long)]
    /// filename from which gateware should be read when write_gateware command
    /// is run
    gateware: Option<PathBuf>,

    #[clap(long)]
    /// path to an ELF file from which MCU program should be read when write_mcu
    /// command is run
    mcu_rom: Option<PathBuf>,

    #[clap(long)]
    /// directory containing test images
    test_data_dir: Option<PathBuf>,

    #[clap(long)]
    /// filename from which SOC ROM should be read when write_soc_rom command is
    /// run
    soc_rom: Option<PathBuf>,

    #[clap(
        long,
        default_value = "grayscale",
        parse(try_from_str = image_type_from_str)
    )]
    /// image type that we expect from the camera
    image_type: ImageType,

    #[clap(long)]
    /// openocd port to which to connect
    openocd_port: u16,

    #[clap(long, default_value = "9000")]
    /// base port to use for RTT channels. This port and the subsequent
    /// 4 ports must not already be in use.
    base_port: u16,

    #[clap(long)]
    /// commands to run on startup
    cmd: Vec<String>,

    #[clap(long)]
    /// don't use i2c via MCP2221 for sending SPI flash data to HPS
    no_mcp_i2c: bool,

    #[clap(long)]
    /// whether to reset MCP2221
    reset_mcp: bool,

    #[clap(long, arg_enum, default_value = "none")]
    /// which interface to use for observing the HPS→MLB interrupt line
    interrupt: InterruptInterface,
}

fn main() -> Result<()> {
    let flags: Flags = Flags::parse();
    let mut device = openocd::OpenOcdDevice::new(flags.openocd_port, flags.base_port)?;

    if flags.reset {
        device.reset()?;
    }

    let hps = if flags.no_mcp_i2c {
        None
    } else {
        let i2c = open_hps_i2c(flags.reset_mcp)?;
        let interrupt: Box<dyn InterruptLine> = match flags.interrupt {
            InterruptInterface::None => Box::new(NoopInterruptLine {}),
            InterruptInterface::AvrFtdiProxy => Box::new(FtdiInterruptListener::open()?),
        };
        let hps: Arc<Mutex<(dyn Hps + Send)>> = Arc::new(Mutex::new(
            HpsAdapter::without_magic_check(Box::new(i2c), interrupt),
        ));
        Some(hps)
    };

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

    let (command_source, console_output) = debug_commands::create_console(&flags.cmd.join(";"))?;

    let channels = create_channels(rtt.from_device, &flags, console_output.clone())?;
    let mut to_device = rtt.to_device.drain(..);
    run_loop(
        &console_output,
        command_source,
        channels,
        to_device.next(),
        to_device.next(),
        cmd_response_channel,
        &flags,
        &mut *device,
        hps,
    )?;
    Ok(())
}

fn open_hps_i2c(reset: bool) -> Result<mcp2221::Handle> {
    let mut config = mcp2221::Config::default();
    config.reset_on_open = reset;
    config.i2c_speed_hz = 400_000;
    let mut dev = mcp2221::Handle::open_first(&config)
        .map_err(|error| anyhow!("Failed to open HPS via I2C: {}", error))?;
    let mut gpio_config = mcp2221::GpioConfig::default();
    gpio_config.set_direction(0, mcp2221::Direction::Output);
    gpio_config.set_value(0, true);
    dev.configure_gpio(&gpio_config)?;
    Ok(dev)
}

/// Create channels. The channels here must match the channel definitions in
/// stage1_app/src/main.rs. e.g. up-channel 0 is the MCU's debug
/// channel, 1 is the FPGA's debug channel.
fn create_channels(
    mut from_device: Vec<Box<dyn DataSource>>,
    flags: &Flags,
    console_output: ConsoleOutput,
) -> Result<Vec<Channel>> {
    let mut raw_channels = from_device.drain(..);
    let mut channels = Vec::new();
    if let Some(up_channel) = raw_channels.next() {
        channels.push(Channel::new(
            up_channel,
            Box::new(TextChannel::new("MCU> ".yellow(), console_output.clone())),
        )?);
    }
    if let Some(up_channel) = raw_channels.next() {
        channels.push(Channel::new(
            up_channel,
            Box::new(TextChannel::new("FPGA> ".green(), console_output.clone())),
        )?);
    }
    if let (Some(image_out), Some(up_channel)) = (&flags.image_out, raw_channels.next()) {
        channels.push(Channel::new(
            up_channel,
            Box::new(image_channel::ImageChannel::new(
                image_out,
                flags.image_type,
                console_output,
            )?),
        )?);
    }
    Ok(channels)
}

fn run_loop(
    console_output: &ConsoleOutput,
    command_source: mpsc::Receiver<Command>,
    mut channels: Vec<Channel>,
    mut mcu_command_channel: Option<Box<dyn DataSink>>,
    mut fpga_command_channel: Option<Box<dyn DataSink>>,
    mut cmd_response_channel: Option<Box<dyn DataSource>>,
    flags: &Flags,
    device: &mut dyn Device,
    hps: Option<Arc<Mutex<dyn Hps + Send>>>,
) -> Result<()> {
    let mut backoff_ms = 0;
    let mut buffer = vec![0; 1024];
    set_blocking_mode(&mut mcu_command_channel, true, console_output)?;
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
                Command::Fpga(c) => {
                    send_command_to_device(&c, &mut fpga_command_channel, console_output)?
                }
                Command::Mcu(c) => {
                    send_command_to_device(&c, &mut mcu_command_channel, console_output)?
                }
                Command::Local(c) => execute_locally(
                    console_output,
                    &c,
                    &mut mcu_command_channel,
                    &mut cmd_response_channel,
                    flags,
                    device,
                    &hps,
                )?,
                Command::Exit => {
                    set_blocking_mode(&mut mcu_command_channel, false, console_output)?;
                    return Ok(());
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

fn set_blocking_mode(
    mcu_command_channel: &mut Option<Box<dyn DataSink>>,
    blocking: bool,
    console_output: &ConsoleOutput,
) -> Result<()> {
    if mcu_command_channel.is_none() {
        return Ok(());
    }
    send_command_to_device(
        &debug_commands::DebugCommand {
            code: mcu_common::McuDebugCommand::SetBlockingMode as u8,
            arg: if blocking { 1 } else { 0 },
        },
        mcu_command_channel,
        console_output,
    )
}

// Executes the given local command
fn execute_locally(
    console_output: &ConsoleOutput,
    c: &LocalCommand,
    mcu_command_channel: &mut Option<Box<dyn DataSink>>,
    cmd_response_channel: &mut Option<Box<dyn DataSource>>,
    flags: &Flags,
    device: &mut dyn Device,
    hps: &Option<Arc<Mutex<dyn Hps + Send>>>,
) -> Result<()> {
    match c.operation {
        LocalOperation::PrintHelp => debug_commands::print_help_text(console_output),
        LocalOperation::ResetMcu => {
            device.reset()?;
            set_blocking_mode(mcu_command_channel, true, console_output)?;
        }
        LocalOperation::WriteMcuProgram => {
            if let Some(path) = &flags.mcu_rom {
                device.write_program(path)?;
            } else {
                console_output.print("--mcu-rom must be specified on startup\n");
            }
        }
        LocalOperation::WriteGateware => {
            if let Some(file) = &flags.gateware {
                if let Err(error) = write_spi_flash::write_file_to_spi_flash(
                    mcu_command_channel,
                    cmd_response_channel,
                    hps.clone(),
                    0,
                    file,
                    console_output,
                ) {
                    console_output.print(format!("{}\n", error));
                }
            } else {
                console_output.print("--gateware must be specified on startup\n");
            }
        }
        LocalOperation::WriteTestImages => {
            if let Some(test_data_dir) = &flags.test_data_dir {
                if let Err(error) = write_spi_flash::write_test_data(
                    mcu_command_channel,
                    cmd_response_channel,
                    hps.clone(),
                    test_data_dir,
                    console_output,
                ) {
                    console_output.print(format!("{}\n", error));
                }
            } else {
                console_output.print("--test-data-dir must be specified on startup\n");
            }
        }
        LocalOperation::WriteSocRom => {
            if let Some(file) = &flags.soc_rom {
                if let Err(error) = write_spi_flash::write_file_to_spi_flash(
                    mcu_command_channel,
                    cmd_response_channel,
                    hps.clone(),
                    SOC_ROM_OFFSET,
                    file,
                    console_output,
                ) {
                    console_output.print(format!("{}\n", error));
                }
            } else {
                console_output.print("--soc-rom must be specified on startup\n");
            }
        }

        LocalOperation::I2cCommand(command) => {
            if let Some(hps) = &hps {
                let mut hps = hps.lock().unwrap();
                hps.perform_command(command)?;
                console_output.print(format!("Command {:?} running\n", command));
                loop {
                    match hps.status() {
                        Ok(status) => {
                            if !status.contains(Status::COMMAND_IN_PROGRESS) {
                                match hps.check_errors() {
                                    Ok(_) => console_output
                                        .print(format!("Command {command:?} complete\n")),
                                    Err(e) => console_output.print(format!(
                                        "Command {command:?} resulted in error: {e}\n"
                                    )),
                                };
                                break;
                            }
                        }
                        Err(e) => {
                            console_output.print(format!("HPS no longer responding on I2C: {e}\n"));
                            break;
                        }
                    }
                }
            } else {
                console_output.print("Cannot perform command without I2C connection\n");
            }
        }
        LocalOperation::SetFeatureEnable => {
            if let Some(hps) = &hps {
                let mut hps = hps.lock().unwrap();
                hps.set_feature_enable(c.arg)?;
            } else {
                console_output.print("Cannot perform command without I2C connection\n");
            }
        }
        LocalOperation::SetI2cRegister(register) => {
            if let Some(hps) = &hps {
                let mut hps = hps.lock().unwrap();
                hps.write_register(register, c.arg)?;
            } else {
                console_output.print("Cannot perform command without I2C connection\n");
            }
        }
        LocalOperation::SleepMilliseconds => {
            std::thread::sleep(Duration::from_millis(c.arg as u64));
        }
    }
    Ok(())
}

fn send_command_to_device(
    command: &debug_commands::DebugCommand,
    command_channel: &mut Option<Box<dyn DataSink>>,
    console_output: &ConsoleOutput,
) -> Result<()> {
    if let Some(command_channel) = command_channel.as_mut() {
        command_channel.write(&command.to_bytes())?;
    } else {
        console_output.print("No command channel available, command not sent.\n");
    }
    Ok(())
}
