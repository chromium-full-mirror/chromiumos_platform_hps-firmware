// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use std::io::Read;
use std::sync::mpsc;

#[derive(Clone, Copy)]
pub(crate) struct DebugCommandCode(pub u8);

struct DebugCommand {
    key: u8,
    help_text: &'static str,
    code: DebugCommandCode,
}

const COMMANDS: &'static [DebugCommand] = &[
    DebugCommand {
        key: b'C',
        help_text: "Disable image classification",
        code: DebugCommandCode(1),
    },
    DebugCommand {
        key: b'c',
        help_text: "Enable image classification",
        code: DebugCommandCode(2),
    },
    DebugCommand {
        key: b'T',
        help_text: "Disable image transfer",
        code: DebugCommandCode(3),
    },
    DebugCommand {
        key: b't',
        help_text: "Enable image transfer",
        code: DebugCommandCode(4),
    },
    DebugCommand {
        key: b'e',
        help_text: "Run exposure calibration",
        code: DebugCommandCode(5),
    },
];

/// Locks stdin and emits debug commands based on keys that get pressed.
/// Terminates when the returned receiver gets closed.
pub(crate) fn create_command_source() -> Result<mpsc::Receiver<DebugCommandCode>> {
    configure_stdin()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        let mut buffer = [0u8; 1];
        loop {
            if matches!(stdin.read(&mut buffer), Ok(1)) {
                if let Some(command) = command_from_key(buffer[0]) {
                    if tx.send(command).is_err() {
                        // Other end of channel has closed.
                        return;
                    }
                }
            }
        }
    });
    Ok(rx)
}

fn configure_stdin() -> Result<()> {
    let stdin = 0;
    let mut term = termios::Termios::from_fd(stdin)?;
    // TODO: Restore terminal on control-c
    term.c_lflag &= !termios::ICANON;
    term.c_lflag &= !termios::ECHO;
    termios::tcsetattr(stdin, termios::TCSANOW, &mut term)?;
    Ok(())
}

fn command_from_key(key: u8) -> Option<DebugCommandCode> {
    for command in COMMANDS {
        if command.key == key {
            return Some(command.code);
        }
    }
    if key != b'?' {
        println!("Unknown key command: '{}'", key as char);
    }
    println!("Available commands: ");
    for command in COMMANDS {
        println!("  {}: {}", command.key as char, command.help_text);
    }
    None
}
