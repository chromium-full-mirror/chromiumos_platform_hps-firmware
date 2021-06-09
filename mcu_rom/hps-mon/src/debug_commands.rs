// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use rustyline::config::Configurer;
use std::sync::mpsc;
use std::time::Duration;

use crate::console::Console;

#[derive(Clone, Copy)]
pub(crate) struct DebugCommand {
    pub(crate) code: u8,
    pub(crate) arg: u16,
}

struct AvailableDebugCommand {
    keyword: &'static str,
    help_text: &'static str,
    command_type: CommandType,
}

enum CommandType {
    FpgaCommand(u8),
    FpgaCommandWithArg(u8),
    McuCommand(u8),
    LocalOperation(LocalOperation),
}

#[derive(Clone, Copy)]
pub(crate) enum LocalOperation {
    PrintHelp,
    Watch,
    Exit,
}

pub(crate) enum Command {
    None,
    FpgaCommand(DebugCommand),
    McuCommand(DebugCommand),
    LocalOperation(LocalOperation),
}

struct CommandEditor {
    editor: rustyline::Editor<CompletionHelper>,
}

#[derive(Default)]
struct CompletionHelper {}

const COMMANDS: &'static [AvailableDebugCommand] = &[
    // TODO: Consider changing commands that set things on and off to single
    // commands with an argument. That would require changing the FPGA side as
    // well, so isn't done initially.
    AvailableDebugCommand {
        keyword: "classification_off",
        help_text: "Disable image classification",
        command_type: CommandType::FpgaCommand(1),
    },
    AvailableDebugCommand {
        keyword: "classification_on",
        help_text: "Enable image classification",
        command_type: CommandType::FpgaCommand(2),
    },
    AvailableDebugCommand {
        keyword: "transfer_off",
        help_text: "Disable image transfer",
        command_type: CommandType::FpgaCommand(3),
    },
    AvailableDebugCommand {
        keyword: "transfer_on",
        help_text: "Enable image transfer",
        command_type: CommandType::FpgaCommand(4),
    },
    AvailableDebugCommand {
        keyword: "exposure_calibration",
        help_text: "Run exposure calibration",
        command_type: CommandType::FpgaCommand(5),
    },
    AvailableDebugCommand {
        keyword: "analog_exposure",
        help_text: "Set analog exposure",
        command_type: CommandType::FpgaCommandWithArg(6),
    },
    AvailableDebugCommand {
        keyword: "digital_exposure",
        help_text: "Set digital exposure",
        command_type: CommandType::FpgaCommandWithArg(7),
    },
    AvailableDebugCommand {
        keyword: "integration",
        help_text: "Set integration",
        command_type: CommandType::FpgaCommandWithArg(8),
    },
    AvailableDebugCommand {
        keyword: "help",
        help_text: "Print command help",
        command_type: CommandType::LocalOperation(LocalOperation::PrintHelp),
    },
    AvailableDebugCommand {
        keyword: "exit",
        help_text: "Exit from hps-mon",
        command_type: CommandType::LocalOperation(LocalOperation::Exit),
    },
    AvailableDebugCommand {
        keyword: "watch",
        help_text: "Watch for output from device",
        command_type: CommandType::LocalOperation(LocalOperation::Watch),
    },
    AvailableDebugCommand {
        keyword: "reset_fpga",
        help_text: "Reset the FPGA",
        command_type: CommandType::McuCommand(1),
    },
];

/// Locks stdin and emits debug commands based on keys that get pressed.
/// Terminates when the returned receiver gets closed.
pub(crate) fn create_command_source(console: Console) -> Result<mpsc::Receiver<Command>> {
    let mut editor = CommandEditor {
        editor: rustyline::Editor::<CompletionHelper>::new(),
    };
    editor.editor.set_helper(Some(CompletionHelper::default()));
    editor
        .editor
        .set_completion_type(rustyline::CompletionType::List);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            let mut command = Command::None;
            console.pause_output_while(|| command = editor.get_command());
            match command {
                Command::None => {}
                Command::FpgaCommand(_) | Command::McuCommand(_) => {
                    if tx.send(command).is_err() {
                        // Other end of channel has closed.
                        return;
                    }
                }
                Command::LocalOperation(LocalOperation::PrintHelp) => {
                    print_help_text();
                }
                Command::LocalOperation(LocalOperation::Exit) => {
                    // TODO: Consider a more graceful way of
                    // exiting. For now this is easy and doesn't
                    // appear to have any real downside.
                    std::process::exit(0)
                }
                Command::LocalOperation(LocalOperation::Watch) => {
                    // Leave console output active while we wait for a line,
                    // which we then ignore.
                    println!("Waiting for messages from device. Press enter to return to prompt.");
                    let _ = editor.editor.readline("");
                }
            }
            // A short delay during which console output is not paused. This
            // gives any output that occurred while we were typing the command,
            // as well as giving a chance for output resulting from the command
            // to get back to us.
            std::thread::sleep(Duration::from_millis(200));
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

impl CommandEditor {
    fn get_command(&mut self) -> Command {
        while let Ok(line) = self.editor.readline(">> ") {
            self.editor.add_history_entry(&line);
            let mut parts = line.split(' ');
            if let Some(keyword) = parts.next() {
                for command in COMMANDS {
                    if command.keyword == keyword {
                        match command.command_type {
                            CommandType::FpgaCommand(code) => {
                                return Command::FpgaCommand(DebugCommand { code, arg: 0 });
                            }
                            CommandType::McuCommand(code) => {
                                return Command::McuCommand(DebugCommand { code, arg: 0 });
                            }
                            CommandType::FpgaCommandWithArg(code) => {
                                if let Some(arg_string) = parts.next() {
                                    if let Some(arg) = parse_value(arg_string) {
                                        return Command::FpgaCommand(DebugCommand { code, arg });
                                    } else {
                                        eprintln!("The argument '{}' couldn't be parsed as a decimal or hex u16", arg_string);
                                    }
                                } else {
                                    eprintln!(
                                        "The command {} requires an argument",
                                        command.keyword
                                    );
                                }
                            }
                            CommandType::LocalOperation(op) => {
                                return Command::LocalOperation(op);
                            }
                        }
                    }
                }
                if keyword.is_empty() {
                    return Command::None;
                }
                eprintln!(
                    "Unknown command '{}'. Type 'help' to see list of commands",
                    keyword
                );
            }
        }
        Command::None
    }
}

fn print_help_text() {
    for command in COMMANDS {
        let arg = if matches!(command.command_type, CommandType::FpgaCommandWithArg(..)) {
            "{arg}"
        } else {
            ""
        };
        println!(
            "{:30} {}",
            format!("{} {}", command.keyword, arg),
            command.help_text
        );
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

impl rustyline::completion::Completer for CompletionHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let prefix = &line[..pos];
        let mut completions = Vec::new();
        for command in COMMANDS {
            if command.keyword.starts_with(prefix) {
                completions.push(command.keyword.to_owned());
            }
        }
        Ok((0, completions))
    }
}

impl rustyline::Helper for CompletionHelper {}
impl rustyline::validate::Validator for CompletionHelper {}
impl rustyline::highlight::Highlighter for CompletionHelper {}
impl rustyline::hint::Hinter for CompletionHelper {
    type Hint = String;
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
