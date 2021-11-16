// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;

/// Controls output to stdout, allowing it to be paused while the user is being
/// prompted for input. This type can be cheaply cloned. All clones refer to the
/// same underlying state.
#[derive(Clone)]
pub(crate) struct Console {
    /// Output requires locking this mutex, so is paused by holding the lock.
    output_lock: Arc<Mutex<()>>,
    line_sink: mpsc::Sender<String>,
}

pub(crate) struct ConsoleController {
    join_handle: JoinHandle<()>,
    console: Console,
}

impl ConsoleController {
    pub(crate) fn new() -> Self {
        let (line_sink, line_source) = mpsc::channel();
        let output_lock = Arc::new(Mutex::new(()));
        let join_handle = std::thread::spawn({
            let output_lock = output_lock.clone();
            move || {
                while let Ok(line) = line_source.recv() {
                    let _guard = output_lock.lock().unwrap();
                    println!("{}", line);
                }
            }
        });
        Self {
            join_handle,
            console: Console {
                output_lock,
                line_sink,
            },
        }
    }

    pub(crate) fn console(&self) -> Console {
        self.console.clone()
    }

    /// Shuts down the thread that handles printing to the console. All
    /// `Console` instances returned by `console` (or cloned from those) must
    /// have been dropped prior to calling this method.
    pub(crate) fn shutdown(self) {
        // There should be exactly 2 references to `output_lock`, one here and
        // one held by the thread created above.
        if Arc::strong_count(&self.console.output_lock) != 2 {
            panic!(
                "Call to ConsoleController::shutdown while there are still externally held \
                Console references"
            );
        }
        // We should have the only instance of Console, which means the only
        // `mpsc::Sender` that is linked to the `mpsc::Receiver` held by the
        // thread. We explicitly drop our console. Once the sender gets dropped,
        // the `recv()` call in the thread will return an error and the thread
        // will exit.
        drop(self.console);
        let _ = self.join_handle.join();
    }
}

impl Console {
    pub(crate) fn new_line_sink(&self) -> mpsc::Sender<String> {
        self.line_sink.clone()
    }

    /// Pauses console output while running `f`.
    pub(crate) fn pause_output_while<T, F: FnOnce() -> T>(&self, f: F) -> T {
        let _guard = self.output_lock.lock().unwrap();
        f()
    }
}
