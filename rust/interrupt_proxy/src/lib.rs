// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use ftdi;
use hps_interface::InterruptLine;
use std::cell::Cell;
use std::io::Read;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;

#[derive(Clone, Copy, Eq, PartialEq)]
enum PinState {
    High,
    Low,
}

// ftdi::Device could be marked as Send, work around it for now.
// TODO(dcallagh): fix it upstream
pub struct D(ftdi::Device);
unsafe impl Send for D {}

/// Listens for bytes from the UART of the ATmega16u4 chip on proto2, which indicate state changes
/// of the MLB interrupt line.
///
/// This assumes the user has already programmed *and started* the AVR.
/// The `scripts/avr-prog` helper script will do this.
pub struct FtdiInterruptListener {
    listen_thread: Option<thread::JoinHandle<()>>, // None while dropping
    receiver: Option<mpsc::Receiver<Option<PinState>>>, // None while dropping
    current_state: Cell<PinState>,
}

impl FtdiInterruptListener {
    pub fn open() -> Result<Self> {
        // HPS proto2 has FT4232H, the AVR is on the D channel.
        let mut ftdi_device = ftdi::find_by_vid_pid(0x0403, 0x6011)
            .interface(ftdi::Interface::D)
            .open()
            .context("Failed opening FT4232H")?;
        ftdi_device.set_baud_rate(9600)?;
        // Set the FTDI "latency timer" as low as we can. This is how many milliseconds the chip
        // itself will buffer bytes before it sends them to us over USB. Since we are always
        // dealing with a single byte at a time, every transfer will be subject to this delay.
        ftdi_device.set_latency_timer(1)?;
        let ftdi_device = Mutex::new(D(ftdi_device));
        let (sender, receiver) = mpsc::channel();
        let listen_thread = Self::spawn(ftdi_device, sender);
        Ok(Self {
            listen_thread: Some(listen_thread),
            receiver: Some(receiver),
            current_state: Cell::new(PinState::High),
        })
    }

    /// Spawns a thread which will read bytes from the FTDI UART and
    /// send interrupt state updates over the given channel.
    fn spawn(
        ftdi_device: Mutex<D>,
        sender: mpsc::Sender<Option<PinState>>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut ftdi_device = ftdi_device.lock().unwrap();
            loop {
                let mut buf = [0u8; 1];
                let next_val = match ftdi_device.0.read(&mut buf) {
                    Ok(1) => match buf[0] {
                        b'!' => Some(PinState::High),
                        b'.' => Some(PinState::Low),
                        _ => panic!("Unexpected byte {:?}", buf[0]),
                    },
                    Ok(0) => None,
                    Ok(_) => unreachable!(),
                    Err(e) => panic!("FTDI read byte failed: {:?}", e),
                };
                // Send Some(state) if the state has changed,
                // or None if there's nothing to report.
                // We always send a value so that we notice promptly
                // when the channel is disconnected.
                if sender.send(next_val).is_err() {
                    // Other end is disconnected, so we shut down
                    break;
                }
            }
        })
    }
}

impl InterruptLine for FtdiInterruptListener {
    fn is_interrupt_asserted(&self) -> Result<bool> {
        // Consume any pending events from the channel, so that we know the most recent state.
        loop {
            match self.receiver.as_ref().unwrap().try_recv() {
                Ok(Some(state)) => {
                    self.current_state.set(state);
                }
                Ok(None) => {}
                Err(mpsc::TryRecvError::Empty) => {
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    bail!("Interrupt listener channel disconnected");
                }
            }
        }
        Ok(self.current_state.get() == PinState::Low)
    }

    fn wait_for_interrupt(&self) -> Result<()> {
        loop {
            match self.receiver.as_ref().unwrap().recv() {
                Ok(Some(state)) => {
                    self.current_state.set(state);
                    if state == PinState::Low {
                        return Ok(());
                    }
                }
                Ok(None) => {}
                Err(mpsc::RecvError) => {
                    bail!("Interrupt listener channel disconnected");
                }
            }
        }
    }
}

impl Drop for FtdiInterruptListener {
    fn drop(&mut self) {
        // Disconnect receiver side so that the listening thread terminates.
        self.receiver.take();
        self.listen_thread
            .take()
            .unwrap()
            .join()
            .expect("FTDI interrupt listening thread failed");
    }
}
