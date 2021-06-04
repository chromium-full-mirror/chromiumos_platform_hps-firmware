// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod console;
mod debug_commands;
mod image_channel;
mod text_channel;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use argh::FromArgs;
use colored::*;
use console::Console;
use image_channel::image_type_from_str;
use image_channel::ImageType;
use probe_rs::Probe;
use probe_rs::Session;
use probe_rs_rtt::ChannelMode;
use probe_rs_rtt::DownChannel;
use probe_rs_rtt::Rtt;
use probe_rs_rtt::UpChannel;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use text_channel::TextChannel;

const DEFAULT_CPU: &str = "stm32g031";

struct Channel {
    up_channel: UpChannel,
    handler: Box<dyn ChannelHandler>,
}

impl Channel {
    fn new(up_channel: UpChannel, handler: Box<dyn ChannelHandler>) -> Result<Self> {
        // Make our channels block on the device side so that we don't miss any
        // messages. This also serves to let the device side know that we're
        // here, otherwise it will send the FPGA output to channel 0 for
        // compatibility with probe-run.
        up_channel.set_mode(ChannelMode::BlockIfFull)?;
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

#[derive(FromArgs)]
/// Monitors the HPS for debug output via SWD.
struct Flags {
    #[argh(option)]
    /// CPU model
    cpu: Option<String>,

    #[argh(switch)]
    /// reset MCU on start
    reset: bool,

    #[argh(option)]
    /// directory into which images should be written
    image_out: Option<PathBuf>,

    #[argh(
        option,
        default = "ImageType::Grayscale",
        from_str_fn(image_type_from_str)
    )]
    /// image type that we expect from the camera
    image_type: ImageType,

    #[argh(option, default = "0")]
    /// which probe number to use, starting from 0.
    probe_num: usize,
}

fn main() -> Result<()> {
    let flags: Flags = argh::from_env();
    let mut session = attach_to_cpu(&flags)?;

    if flags.reset {
        // The HPS MCU only has a single core, core 0.
        let mut core = session.core(0)?;
        core.reset()?;
    }

    let console = Console::new();
    let session = Arc::new(Mutex::new(session));
    let mut rtt = attach_rtt(&session, 10)?;

    let channels = create_channels(&mut rtt, &flags, console.new_line_sink())?;
    run_loop(channels, rtt.down_channels().take(0), console)
}

fn attach_to_cpu(flags: &Flags) -> Result<Session> {
    let probes = Probe::list_all();
    if probes.is_empty() {
        bail!("No supported probes were detected");
    }
    let probe = probes
        .get(flags.probe_num)
        .ok_or_else(|| {
            anyhow!(
                "Probe #{} not found. Only {} probes were detected",
                flags.probe_num,
                probes.len()
            )
        })?
        .open()?;
    Ok(probe.attach(
        flags
            .cpu
            .as_ref()
            .map(String::as_str)
            .unwrap_or(DEFAULT_CPU),
    )?)
}

/// Create channels. The channels here must match the channel definitions in
/// stm32g0_application/src/main.rs. e.g. up-channel 0 is the MCU's debug
/// channel, 1 is the FPGA's debug channel.
fn create_channels(
    rtt: &mut Rtt,
    flags: &Flags,
    line_sink: mpsc::Sender<String>,
) -> Result<Vec<Channel>> {
    let mut channels = Vec::new();
    if let Some(up_channel) = rtt.up_channels().take(0) {
        channels.push(Channel::new(
            up_channel,
            Box::new(TextChannel::new("MCU> ".yellow(), line_sink.clone())),
        )?);
    }
    if let Some(up_channel) = rtt.up_channels().take(1) {
        channels.push(Channel::new(
            up_channel,
            Box::new(TextChannel::new("FPGA> ".green(), line_sink.clone())),
        )?);
    }
    if let (Some(image_out), Some(up_channel)) = (&flags.image_out, rtt.up_channels().take(2)) {
        channels.push(Channel::new(
            up_channel,
            Box::new(image_channel::ImageChannel::new(
                image_out,
                flags.image_type,
                line_sink.clone(),
            )?),
        )?);
    }
    Ok(channels)
}

fn run_loop(
    mut channels: Vec<Channel>,
    command_channel: Option<DownChannel>,
    console: Console,
) -> Result<()> {
    let mut backoff_ms = 0;
    let mut buffer = vec![0; 1024];
    let command_source = debug_commands::create_command_source(console)?;
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
        if let Ok(command) = command_source.try_recv() {
            if let Some(command_channel) = command_channel.as_ref() {
                command_channel.write(&command.to_bytes())?;
            } else {
                eprintln!("No command channel available, command not sent.");
            }
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

fn attach_rtt(session: &Arc<Mutex<Session>>, mut retries: u32) -> Result<Rtt> {
    // Attaching to RTT may take several tries if we've just reset the device,
    // since the control block that RTT looks for may not have been initialized.
    loop {
        match Rtt::attach(Arc::clone(session)) {
            Ok(rtt) => return Ok(rtt),
            Err(e) => {
                if retries == 0 {
                    return Err(e.into());
                }
                retries -= 1;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
