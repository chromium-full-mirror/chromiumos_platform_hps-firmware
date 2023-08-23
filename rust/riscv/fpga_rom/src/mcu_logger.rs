// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use core::fmt::Write;
use fpga_app::McuInterface;

struct McuLogger;

static LOGGER: McuLogger = McuLogger;

pub(crate) fn init_logging() {
    // SAFETY: Safe because we have no concurrency (single core and no
    // interrupts), so there's nothing to race.
    unsafe {
        log::set_logger_racy(&LOGGER).unwrap();
        log::set_max_level_racy(log::LevelFilter::Info);
    }
}

impl log::Log for McuLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::LevelFilter::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            // SAFETY: We conjure a zero-sized type out of nothing. Since it's
            // zero-sized, we can't get the representation wrong. Plausibly, if
            // logging were to be used in McuInterface, while it's doing stuff
            // with the SPI registers, we could end up with the SPI peripheral
            // registers in an unexpected state. It's not unsound though because
            // all reads and writes are volatile. More likely if logging were
            // used in say McuInterface::exchange, we'd end up with a stack
            // overflow.
            let mut mcu: McuInterface<fpga_hal::McuSpi> = unsafe { core::mem::transmute(()) };
            // Not much we can do if we fail to log here, so ignore any errors.
            let _ = writeln!(&mut mcu, "{}: {}", record.metadata().level(), record.args());
        }
    }

    fn flush(&self) {}
}
