// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hps_interface::Hps;
use mcu_common::CommonHostInterface;

#[derive(Default)]
pub struct FakeHps {
    common: CommonHostInterface,
    reg: Option<(u8, u16)>, // (reg, val).
}

impl Hps for FakeHps {
    fn read_register(&mut self, reg: u8) -> anyhow::Result<u16> {
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

    fn write_register(&mut self, reg: u8, value: u16) -> anyhow::Result<()> {
        self.reg = Some((reg, value));
        Ok(())
    }

    fn write_memory(&mut self, _bank: u8, _address: u32, _values: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
}
