// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;

use application::fpga::Action;
use application::fpga::Request;
use application::ApplicationState;
use embedded_hal::blocking::i2c;
use embedded_hal::blocking::spi;
use mcu_common::MemBlock;

#[derive(Default)]
pub(crate) struct FakeMcu {
    pub(crate) state: ApplicationState,
    pub(crate) camera: FakeCamera,
    last_action: Option<Action>,
    last_action_count: u32,
}

#[derive(Default)]
pub(crate) struct FakeCamera {
    registers: HashMap<u16, u8>,
}

pub(crate) struct FakeMcuError;

impl FakeCamera {
    fn write_register(&mut self, register: u16, value: u8) {
        self.registers.insert(register, value);
    }

    fn read_register(&mut self, register: u16) -> u8 {
        self.registers.get(&register).cloned().unwrap_or(0)
    }
}

impl spi::Transfer<u8> for FakeMcu {
    type Error = FakeMcuError;

    fn transfer<'w>(&mut self, words: &'w mut [u8]) -> Result<&'w [u8], Self::Error> {
        // We reuse parts of the actuall MCU application. It can't work directly
        // with `words`, so we construct a MemBlock, copy our data into the
        // memblock, then copy the response back when we're done.
        let mut buffer = MemBlock::with_capacity(words.len());
        buffer.copy_from_slice(words);
        let request = Request::from_received_packet(&buffer);
        match request.action {
            Action::Poll => {}
            Action::ReadCameraI2c => {
                use i2c::WriteRead;
                // Each pair of bytes is a register number. Result is stored
                // back into the fist byte.
                for chunk in buffer[2..].chunks_exact_mut(2) {
                    let mut out = [0u8];
                    let _ = self.camera.write_read(0, chunk, &mut out);
                    chunk[0] = out[0];
                }
            }
            Action::WriteCameraI2c => {
                use i2c::Write;
                // Each 3 bytes is a 2-byte register and a 1 byte value.
                for chunk in buffer[2..].chunks_exact_mut(3) {
                    self.camera.write(0, chunk)?;
                }
            }
            Action::ReadConfigRegisters => {
                application::fpga::read_config_registers(&mut buffer, &self.state);
            }
            Action::WriteStatusRegisters => {
                application::fpga::write_status_registers(&buffer, &mut self.state);
            }
            Action::ReportFpgaBoot => {
                application::fpga::report_boot(&mut self.state, &buffer);
            }
            Action::ReportFpgaError => {}
            Action::TriggerFrame => {}
            Action::Echo => {}
            _ => panic!("Unimplemented fake of {:?}", request.action),
        }
        request.prepare_to_send(&mut buffer);
        words.copy_from_slice(&buffer);

        // Make sure we're not stuck repeatedly sending the same request over
        // and over. Generally if this happens it's because the FPGA is polling
        // the MCU waiting for something to happen, but we haven't got the
        // appropriate implementation here on FakeMcu.
        if self.last_action == Some(request.action) {
            self.last_action_count += 1;
            if self.last_action_count > 10 {
                panic!(
                    "Test appears to be stuck repeatedly sending: {:?}",
                    request.action
                );
            }
        } else {
            self.last_action = Some(request.action);
            self.last_action_count = 1;
        }

        Ok(words)
    }
}

impl i2c::WriteRead for FakeCamera {
    type Error = FakeMcuError;

    fn write_read(
        &mut self,
        _address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        if buffer.len() == 1 {
            buffer[0] = self.read_register(u16::from_be_bytes([bytes[0], bytes[1]]));
        }
        Ok(())
    }
}

impl i2c::Write for FakeCamera {
    type Error = FakeMcuError;

    fn write(&mut self, _address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes.len() == 3 {
            self.write_register(u16::from_be_bytes([bytes[0], bytes[1]]), bytes[2]);
        }
        Ok(())
    }
}
