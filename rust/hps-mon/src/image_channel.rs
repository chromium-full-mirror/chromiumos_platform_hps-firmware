// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::debug_commands::ConsoleOutput;
use crate::ChannelHandler;
use anyhow::Result;
use host_dev_common::images::ImageType;
use host_dev_common::images::OutputDir;
use std::path::Path;

/// Receives images and writes them out to disk in RAW format and PNG.
pub(crate) struct ImageChannel {
    received: Vec<u8>,
    output_dir: OutputDir,
}

impl ImageChannel {
    pub(crate) fn new(
        out_dir: &Path,
        image_type: ImageType,
        console_output: ConsoleOutput,
    ) -> Result<Self> {
        Ok(Self {
            received: Vec::new(),
            output_dir: OutputDir::new(
                out_dir.to_owned(),
                image_type,
                Box::new(move |log_line| {
                    console_output.print(log_line);
                    Ok(())
                }),
            )?,
        })
    }
}

impl ChannelHandler for ImageChannel {
    fn handle(&mut self, bytes: &[u8]) -> Result<()> {
        self.received.extend_from_slice(bytes);
        self.output_dir
            .process_images_from_buffer(&mut self.received)
    }
}
