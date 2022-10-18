// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::DataSink;
use crate::DataSource;
use crate::Device;
use crate::RttChannels;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

pub(crate) struct OpenOcdDevice {
    connection: TcpStream,
    next_port: u16,
}

#[derive(Clone)]
struct RttChannel {
    connection: Arc<Mutex<TcpStream>>,
}

impl OpenOcdDevice {
    pub(crate) fn new(port: u16, base_port: u16) -> Result<Box<dyn Device>> {
        Ok(Box::new(Self {
            connection: connect_local(port)?,
            next_port: base_port,
        }))
    }

    fn open_rtt_channel(&mut self, channel: u16) -> Result<RttChannel> {
        let port = self.next_port;
        self.connection
            .write_all(format!("rtt server start {} {}\n", port, channel).as_bytes())
            .map_err(|error| anyhow!("Failed writing to OpenOCD control channel: {}", error))?;
        self.connection.flush()?;
        self.next_port += 1;
        let rtt_connection = connect_local_with_retry(port)?;
        // For now, we put our connections into non-blocking mode then poll
        // them. This allows us to match the model that we have with an ST-Link,
        // where we need to poll. It's a teensy bit wasteful though, since we
        // don't need to poll, since OpenOCD is already doing the polling for
        // us. For now though, it's simplest if everything polls.
        rtt_connection
            .set_nonblocking(true)
            .map_err(|error| anyhow!("Failed to set non-blocking mode: {}", error))?;
        Ok(RttChannel {
            connection: Arc::new(Mutex::new(rtt_connection)),
        })
    }
}

impl Device for OpenOcdDevice {
    fn reset(&mut self) -> anyhow::Result<()> {
        self.connection.write_all(b"reset\n")?;
        Ok(())
    }

    fn attach_rtt(&mut self) -> anyhow::Result<RttChannels> {
        // Initialize RTT. OpenOCD will scan the specified memory range looking
        // for the RTT controll block. With flip-link active, this will likely
        // be towards the end of RAM.
        const MCU_RAM_BASE_ADDRESS: u32 = 0x20000000;
        const MCU_RAM_SIZE: u32 = 36 * 1024;
        self.connection.write_all(
            format!(
                r#"rtt setup {} {} "SEGGER RTT"
        rtt start
        "#,
                MCU_RAM_BASE_ADDRESS, MCU_RAM_SIZE
            )
            .as_bytes(),
        )?;
        let mut from_device: Vec<Box<dyn DataSource>> = vec![];
        let mut to_device: Vec<Box<dyn DataSink>> = vec![];
        for channel_num in 0..4 {
            let channel = Box::new(self.open_rtt_channel(channel_num)?);
            from_device.push(channel.clone());
            to_device.push(channel);
        }
        Ok(RttChannels {
            from_device,
            to_device,
        })
    }

    fn write_program(&mut self, filename: &Path) -> Result<()> {
        self.connection
            .write_all(format!("program {} verify reset\n", filename.display()).as_bytes())?;
        Ok(())
    }
}

fn connect_local_with_retry(port: u16) -> Result<TcpStream> {
    for _ in 0..100 {
        if let Ok(connection) = connect_local(port) {
            return Ok(connection);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    connect_local(port)
}

fn connect_local(port: u16) -> Result<TcpStream> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect(address).with_context(|| {
        format!(
            "Failed to connect to localhost on port {port}\n\
             Perhaps you need to run ./scripts/proto2-openocd"
        )
    })
}

impl DataSource for RttChannel {
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize> {
        match self.connection.lock().unwrap().read(bytes) {
            Ok(count) => Ok(count),
            Err(error) => {
                if matches!(error.kind(), ErrorKind::WouldBlock) {
                    Ok(0)
                } else {
                    bail!("Error reading from RTT channel: {}", error);
                }
            }
        }
    }

    fn set_read_blocking(&mut self, blocking: bool) -> Result<()> {
        self.connection.lock().unwrap().set_nonblocking(!blocking)?;
        Ok(())
    }

    fn set_blocking_mode(&mut self) -> Result<()> {
        // Unfortunately OpenOCD doesn't support changing RTT channel modes.
        Ok(())
    }
}

impl DataSink for RttChannel {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        connection.write_all(bytes)?;
        connection.flush()?;
        Ok(())
    }
}
