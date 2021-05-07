// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use hps_interface::Hps;
use mcu_common::CommonHostInterface;

#[derive(Default)]
pub struct FakeHps {
    common: CommonHostInterface,
}

impl Hps for FakeHps {
    fn read_register(&mut self, reg: u8) -> anyhow::Result<u16> {
        Ok(self.common.read_register(reg).unwrap_or(0))
    }

    fn write_register(&mut self, _reg: u8, _value: u16) -> anyhow::Result<()> {
        Ok(())
    }

    fn write_memory(&mut self, _bank: u8, _address: u32, _values: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
}
