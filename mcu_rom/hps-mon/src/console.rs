// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;

/// Controls output to stdout, allowing it to be paused while the user is being
/// prompted for input. This type can be cheaply cloned. All clones refer to the
/// same underlying state.
#[derive(Clone)]
pub(crate) struct Console {
    /// Holds console state. Output requires locking this mutex, so is paused by
    /// holding the lock.
    thread: Arc<Mutex<ConsoleState>>,
    line_sink: mpsc::Sender<String>,
}

/// Wrapper for a thread that joins on drop.
struct ConsoleState {
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Console {
    pub(crate) fn new() -> Self {
        let (line_sink, line_source) = mpsc::channel();
        let inner = Arc::new(Mutex::new(ConsoleState { thread: None }));
        let thread = std::thread::spawn({
            let inner = inner.clone();
            move || {
                while let Ok(line) = line_source.recv() {
                    let _guard = inner.lock().unwrap();
                    println!("{}", line);
                }
            }
        });
        inner.lock().unwrap().thread = Some(thread);
        Self {
            thread: inner,
            line_sink,
        }
    }

    pub(crate) fn new_line_sink(&self) -> mpsc::Sender<String> {
        self.line_sink.clone()
    }

    /// Pauses console output while running `f`.
    pub(crate) fn pause_output_while<T, F: FnOnce() -> T>(&self, f: F) -> T {
        let _guard = self.thread.lock().unwrap();
        f()
    }
}

impl Drop for ConsoleState {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
