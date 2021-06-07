// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::Flags;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use probe_rs::Probe;
use probe_rs::Session;
use probe_rs_rtt::Rtt;

const DEFAULT_CPU: &str = "stm32g031";

pub(crate) fn attach_to_cpu(flags: &Flags) -> Result<Session> {
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

pub(crate) fn attach_rtt(session: &Arc<Mutex<Session>>, mut retries: u32) -> Result<Rtt> {
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
