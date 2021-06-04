// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use std::io::Read;
use std::sync::mpsc;

use crate::console::Console;

#[derive(Clone, Copy)]
pub(crate) struct DebugCommand {
    pub(crate) code: u8,
    pub(crate) arg: u16,
}

struct AvailableDebugCommand {
    key: u8,
    help_text: &'static str,
    code: u8,
    needs_arg: bool,
}

struct CommandEditor {
    editor: rustyline::Editor<()>,
    console: Console,
}

const COMMANDS: &'static [AvailableDebugCommand] = &[
    AvailableDebugCommand {
        key: b'C',
        help_text: "Disable image classification",
        code: 1,
        needs_arg: false,
    },
    AvailableDebugCommand {
        key: b'c',
        help_text: "Enable image classification",
        code: 2,
        needs_arg: false,
    },
    AvailableDebugCommand {
        key: b'T',
        help_text: "Disable image transfer",
        code: 3,
        needs_arg: false,
    },
    AvailableDebugCommand {
        key: b't',
        help_text: "Enable image transfer",
        code: 4,
        needs_arg: false,
    },
    AvailableDebugCommand {
        key: b'e',
        help_text: "Run exposure calibration",
        code: 5,
        needs_arg: false,
    },
    AvailableDebugCommand {
        key: b'a',
        help_text: "Set analog exposure",
        code: 6,
        needs_arg: true,
    },
    AvailableDebugCommand {
        key: b'd',
        help_text: "Set digital exposure",
        code: 7,
        needs_arg: true,
    },
    AvailableDebugCommand {
        key: b'i',
        help_text: "Set integration",
        code: 8,
        needs_arg: true,
    },
];

/// Locks stdin and emits debug commands based on keys that get pressed.
/// Terminates when the returned receiver gets closed.
pub(crate) fn create_command_source(console: Console) -> Result<mpsc::Receiver<DebugCommand>> {
    configure_stdin()?;
    let mut editor = CommandEditor {
        editor: rustyline::Editor::<()>::new(),
        console,
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        let mut buffer = [0u8; 1];
        loop {
            if matches!(stdin.read(&mut buffer), Ok(1)) {
                if let Some(command) = editor.command_from_key(buffer[0]) {
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

impl DebugCommand {
    pub(crate) fn to_bytes(&self) -> [u8; DEBUG_BYTES_PER_COMMAND] {
        let mut result = [0u8; DEBUG_BYTES_PER_COMMAND];
        result[0] = mcu_common::DEBUG_COMMAND_START;
        result[1] = self.code;
        result[2..4].clone_from_slice(&self.arg.to_le_bytes());
        result
    }
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

impl CommandEditor {
    fn command_from_key(&mut self, key: u8) -> Option<DebugCommand> {
        for command in COMMANDS {
            if command.key == key {
                let mut arg = 0;
                if command.needs_arg {
                    if let Some(v) = self.prompt_for_value() {
                        arg = v;
                    } else {
                        return None;
                    }
                }
                return Some(DebugCommand {
                    code: command.code,
                    arg,
                });
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

    fn prompt_for_value(&mut self) -> Option<u16> {
        let editor = &mut self.editor;
        self.console
            .pause_output_while(move || editor.readline("Enter value: ").ok().and_then(parse_value))
    }
}

/// Returns `value` parsed as either a decimal, or if it starts with "0x", then
/// as hex.
fn parse_value<T: AsRef<str>>(value: T) -> Option<u16> {
    let value = value.as_ref();
    if value.starts_with("0x") {
        u16::from_str_radix(value.trim_start_matches("0x"), 16).ok()
    } else {
        value.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_value() {
        assert_eq!(parse_value("32"), Some(32));
        assert_eq!(parse_value("0x10"), Some(16));
    }
}
