// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::DataSink;
use crate::DataSource;
use crate::Device;
use crate::Flags;
use crate::RttChannels;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use probe_rs::Probe;
use probe_rs::Session;
use probe_rs_rtt::ChannelMode;
use probe_rs_rtt::DownChannel;
use probe_rs_rtt::Rtt;
use probe_rs_rtt::UpChannel;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

const DEFAULT_CPU: &str = "stm32g031";

pub(crate) struct DirectDevice {
    session: Arc<Mutex<Session>>,
}

struct DirectUpChannel {
    up_channel: UpChannel,
}

struct DirectDownChannel {
    down_channel: DownChannel,
}

impl DirectDevice {
    pub(crate) fn new(flags: &Flags) -> Result<Box<dyn Device>> {
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
        let session = probe.attach(
            flags
                .cpu
                .as_ref()
                .map(String::as_str)
                .unwrap_or(DEFAULT_CPU),
        )?;
        Ok(Box::new(DirectDevice {
            session: Arc::new(Mutex::new(session)),
        }))
    }
}

impl Device for DirectDevice {
    fn reset(&mut self) -> Result<()> {
        // The HPS MCU only has a single core, core 0.
        let mut session = self.session.lock().unwrap();
        let mut core = session.core(0)?;
        core.reset()?;
        Ok(())
    }

    fn attach_rtt(&mut self) -> Result<RttChannels> {
        // Attaching to RTT may take several tries if we've just reset the device,
        // since the control block that RTT looks for may not have been initialized.
        let mut retries = 10;
        loop {
            match Rtt::attach(Arc::clone(&self.session)) {
                Ok(rtt) => return Ok(get_all_channels(rtt)),
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
}

impl DataSource for DirectUpChannel {
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize> {
        Ok(self.up_channel.read(bytes)?)
    }

    fn set_blocking_mode(&mut self) -> Result<()> {
        self.up_channel.set_mode(ChannelMode::BlockIfFull)?;
        Ok(())
    }
}

impl DataSink for DirectDownChannel {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.down_channel.write(bytes)?;
        Ok(())
    }
}

fn get_all_channels(mut rtt: Rtt) -> RttChannels {
    RttChannels {
        from_device: rtt
            .up_channels()
            .drain()
            .map(|up_channel| -> Box<dyn DataSource> { Box::new(DirectUpChannel { up_channel }) })
            .collect(),
        to_device: rtt
            .down_channels()
            .drain()
            .map(|down_channel| -> Box<dyn DataSink> {
                Box::new(DirectDownChannel { down_channel })
            })
            .collect(),
    }
}
