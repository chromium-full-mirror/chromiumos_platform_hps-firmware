// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use hps_interface::Hps;
use mcu_common::registers::Register;
use mcu_common::CommonHostInterface;

#[derive(Default)]
pub struct FakeHps {
    common: CommonHostInterface,
    reg: Option<(Register, u16)>, // (reg, val).
}

impl Hps for FakeHps {
    fn is_interrupt_asserted(&mut self) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn wait_for_interrupt(&mut self) -> anyhow::Result<()> {
        bail!("Tried to wait for an interrupt that will never come");
    }

    fn read_register(&mut self, reg: Register) -> anyhow::Result<u16> {
        let ret = match self.reg {
            Some((r, v)) => {
                if r == reg {
                    v
                } else {
                    self.common.read_register(reg).unwrap_or(0)
                }
            }
            None => self.common.read_register(reg).unwrap_or(0),
        };
        Ok(ret)
    }

    fn read_register_bytes(&mut self, _reg: Register, _length: usize) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }

    fn write_register(&mut self, reg: Register, value: u16) -> anyhow::Result<()> {
        self.reg = Some((reg, value));
        Ok(())
    }

    fn write_memory(&mut self, _bank: u8, _address: u32, _values: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn write_unchecked(&mut self, _bytes: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn write_read_unchecked(
        &mut self,
        _bytes: &[u8],
        _read_length: usize,
    ) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }
}
