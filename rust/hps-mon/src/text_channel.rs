// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::sync::mpsc;

use crate::ChannelHandler;
use anyhow::Result;
use colored::ColoredString;

/// Receives text in UTF-8 and prints out each line with a source-specific
/// prefix.
pub(crate) struct TextChannel {
    received: Vec<u8>,
    line_prefix: ColoredString,
    lines_out: mpsc::Sender<String>,
}

impl TextChannel {
    pub(crate) fn new(line_prefix: ColoredString, lines_out: mpsc::Sender<String>) -> Self {
        Self {
            received: Vec::new(),
            line_prefix,
            lines_out,
        }
    }
}

impl ChannelHandler for TextChannel {
    fn handle(&mut self, bytes: &[u8]) -> Result<()> {
        self.received.extend_from_slice(bytes);
        if let Some((last_newline, _)) = self
            .received
            .iter()
            .enumerate()
            .rev()
            .find(|(_, byte)| **byte == b'\n')
        {
            if let Ok(message) = std::str::from_utf8(&self.received[..last_newline]) {
                for line in message.lines() {
                    self.lines_out
                        .send(format!("{}{}", self.line_prefix, line))?;
                }
                self.received.drain(..last_newline + 1);
            } else {
                self.lines_out.send("Device sent invalid UTF-8".into())?;
            }
        }
        Ok(())
    }
}
