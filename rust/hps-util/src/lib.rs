// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod fake_hps;
mod send_i2c;

use anyhow::Result;
pub use fake_hps::FakeHps;
use hps_interface::Hps;
pub use send_i2c::SendI2c;

pub fn query_status(hps: &mut dyn Hps) -> Result<String> {
    match hps.check_errors() {
        Ok(_) => Ok("No errors reported".to_string()),
        Err(error) => Ok(error.to_string()),
    }
}
