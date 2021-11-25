// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

const RING_BUFFER_SIZE: usize = 0x5;

pub enum Subsystem {
    Camera = 0x1,
    Flash,
    FPGA,
    MCU,
}

#[derive(Default)]
struct State {
    idx: u16,
    val: u16,
}

#[derive(Default)]
pub struct Trace {
    counter: usize,
    state: [State; RING_BUFFER_SIZE],
}

impl Trace {
    pub fn new() -> Self {
        Trace::default()
    }
    pub fn get_idx(&self) -> u16 {
        self.state[self.counter].idx
    }
    pub fn get_val(&self) -> u16 {
        self.state[self.counter].val
    }
    pub fn trace(&mut self, ss: Subsystem, device: u8, val: u16) {
        self.state[self.counter % RING_BUFFER_SIZE].idx = encode_idx(self.counter, ss, device);
        self.state[self.counter % RING_BUFFER_SIZE].val = val;
        self.counter += 1;
    }
}

fn encode_idx(counter: usize, ss: Subsystem, device: u8) -> u16 {
    ((counter as u16) << 8) | ((ss as u8) << 4 | device) as u16
}
