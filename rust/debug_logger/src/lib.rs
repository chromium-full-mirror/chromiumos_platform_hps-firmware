// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Logger for use by HPS binaries.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

/// Initializes logging, provided the dev feature is enabled.
///
/// # Safety
///
/// Caller must ensure that this function is called during initialization when
/// interrupts are disabled.
pub unsafe fn initialize_logging() {
    #[cfg(feature = "dev")]
    {
        // SAFETY: Safe because interrupts are disabled.
        log::set_logger_racy(&rtt_logger::LOGGER).unwrap();
        log::set_max_level_racy(log::LevelFilter::Info);
    }
}

#[cfg(feature = "dev")]
mod rtt_logger {
    pub(crate) struct RttLogger;

    pub(crate) static LOGGER: RttLogger = RttLogger;

    impl log::Log for RttLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::LevelFilter::Info
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                rtt_target::rprintln!("{}: {}", record.metadata().level(), record.args());
            }
        }

        fn flush(&self) {}
    }
}
