// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::bail;
use anyhow::Result;
use mcu_common::McuDebugCommand;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use rustyline::config::Configurer;
use std::sync::mpsc;

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
    McuCommand(McuDebugCommand),
    McuCommandWithArg(McuDebugCommand),
    LocalOperation(LocalOperation),
}

#[derive(Clone, Copy)]
pub(crate) enum LocalOperation {
    PrintHelp,
    Exit,
    ResetMcu,
    WriteGateware,
    WriteSocRom,
}

pub(crate) enum Command {
    FpgaCommand(DebugCommand),
    McuCommand(DebugCommand),
    LocalOperation(LocalOperation),
}

struct CommandEditor {
    editor: rustyline::Editor<CompletionHelper>,
    should_exit: bool,
}

#[derive(Default)]
struct CompletionHelper {}

const COMMANDS: &[AvailableDebugCommand] = &[
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
        keyword: "camera_custom_address",
        help_text: "Set custom register address for subsequent camera_write_custom command",
        command_type: CommandType::FpgaCommandWithArg(9),
    },
    AvailableDebugCommand {
        keyword: "camera_custom_write",
        help_text: "Write camera register specified by last camera_custom_address",
        command_type: CommandType::FpgaCommandWithArg(10),
    },
    AvailableDebugCommand {
        keyword: "camera_read_register",
        help_text: "Read specified camera register",
        command_type: CommandType::FpgaCommandWithArg(11),
    },
    AvailableDebugCommand {
        keyword: "automatic_exposure_target",
        help_text: "Set automatic exposure target (0 to disable AE)",
        command_type: CommandType::FpgaCommandWithArg(13),
    },
    AvailableDebugCommand {
        keyword: "read_camera_id_fpga",
        help_text: "Read Camera ID FPGA <spi> MCU <i2c> Camera",
        command_type: CommandType::FpgaCommand(15),
    },
    AvailableDebugCommand {
        keyword: "rotate_image_clockwise_off",
        help_text: "Disable image rotation clockwise (for vertical camera mount)",
        command_type: CommandType::FpgaCommand(16),
    },
    AvailableDebugCommand {
        keyword: "rotate_image_clockwise_on",
        help_text: "Enable image rotation clockwise (for horizontal camera mount)",
        command_type: CommandType::FpgaCommand(17),
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
        keyword: "reset_fpga",
        help_text: "Reset the FPGA",
        command_type: CommandType::McuCommand(McuDebugCommand::ResetFpga),
    },
    AvailableDebugCommand {
        keyword: "read_camera_id",
        help_text: "Reads camera ID over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadCameraId),
    },
    AvailableDebugCommand {
        keyword: "read_camera_ae_config",
        help_text: "Reads camera AE config over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadCameraAEConfig),
    },
    AvailableDebugCommand {
        keyword: "read_camera_integration_gains",
        help_text: "Reads camera AE config over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadCameraIntegrationGains),
    },
    AvailableDebugCommand {
        keyword: "set_camera_digital_gain",
        help_text: "Sets camera digital gain over I2C",
        command_type: CommandType::McuCommandWithArg(McuDebugCommand::SetCameraDigitalGain),
    },
    AvailableDebugCommand {
        keyword: "set_camera_analog_gain",
        help_text: "Sets camera analog gain over I2C",
        command_type: CommandType::McuCommandWithArg(McuDebugCommand::SetCameraAnalogGain),
    },
    AvailableDebugCommand {
        keyword: "set_camera_integration",
        help_text: "Sets camera integration over I2C",
        command_type: CommandType::McuCommandWithArg(McuDebugCommand::SetCameraIntegration),
    },
    AvailableDebugCommand {
        keyword: "reset_mcu",
        help_text: "Reset the MCU",
        command_type: CommandType::LocalOperation(LocalOperation::ResetMcu),
    },
    AvailableDebugCommand {
        keyword: "ping_mcu",
        help_text: "Ask the MCU to print something",
        command_type: CommandType::McuCommand(McuDebugCommand::Ping),
    },
    AvailableDebugCommand {
        keyword: "read_spi_flash",
        help_text: "Ask the MCU to read part of the spi flash and print it",
        command_type: CommandType::McuCommandWithArg(McuDebugCommand::ReadSpiFlash),
    },
    AvailableDebugCommand {
        keyword: "write_gateware",
        help_text: "Write gateware to SPI flash",
        command_type: CommandType::LocalOperation(LocalOperation::WriteGateware),
    },
    AvailableDebugCommand {
        keyword: "write_soc_rom",
        help_text: "Write soft CPU binary to SPI flash",
        command_type: CommandType::LocalOperation(LocalOperation::WriteSocRom),
    },
    AvailableDebugCommand {
        keyword: "fpga_hold_in_reset",
        help_text: "Put the FPGA into reset and leave it that way",
        command_type: CommandType::McuCommand(McuDebugCommand::FpgaHoldInReset),
    },
    AvailableDebugCommand {
        keyword: "erase_spi_flash",
        help_text: "Erase entire SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::EraseSpiFlash),
    },
    AvailableDebugCommand {
        keyword: "hash_spi_flash",
        help_text: "Hash part of the SPI flash",
        command_type: CommandType::McuCommandWithArg(McuDebugCommand::HashSpiFlash),
    },
    AvailableDebugCommand {
        keyword: "spi_flash_read_speed",
        help_text: "Read 1MB of SPI flash and report speed",
        command_type: CommandType::McuCommand(McuDebugCommand::SpiFlashReadSpeed),
    },
    AvailableDebugCommand {
        keyword: "spi_flash_write_speed",
        help_text: "Write 1MB of SPI flash and report speed",
        command_type: CommandType::McuCommand(McuDebugCommand::SpiFlashWriteSpeed),
    },
    AvailableDebugCommand {
        keyword: "fpga_power_off",
        help_text: "Power off the FPGA and SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::FpgaPowerOff),
    },
    AvailableDebugCommand {
        keyword: "fpga_power_on",
        help_text: "Power on the FPGA and SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::FpgaPowerOn),
    },
];

/// Locks stdin and emits debug commands based on keys that get pressed.
/// Terminates when the returned receiver gets closed.
pub(crate) fn create_command_source(
    console: Console,
    initial_commands: &str,
) -> Result<mpsc::Receiver<Command>> {
    let mut editor = CommandEditor {
        editor: rustyline::Editor::<CompletionHelper>::new(),
        should_exit: false,
    };
    editor.editor.set_helper(Some(CompletionHelper::default()));
    editor
        .editor
        .set_completion_type(rustyline::CompletionType::List);
    let (tx, rx) = mpsc::channel();
    if let Ok(commands) = parse_commands(initial_commands) {
        for command in commands {
            let is_exit = matches!(command, Command::LocalOperation(LocalOperation::Exit));
            if tx.send(command).is_err() || is_exit {
                return Ok(rx);
            }
        }
    }
    std::thread::spawn(move || {
        println!("Welcome to hps-mon. Press enter to activate console.");
        loop {
            editor.wait_for_enter();
            let mut commands = vec![];
            console.pause_output_while(|| commands = editor.get_commands());
            for command in commands {
                let is_exit = matches!(command, Command::LocalOperation(LocalOperation::Exit));
                if tx.send(command).is_err() {
                    // Other end of channel has closed.
                    return;
                }
                if is_exit {
                    // We need to explicitly handle exit here, otherwise
                    // we'll prompt the user for another line of input
                    // before we discover that the channel has been closed
                    // by the main thread.
                    return;
                }
            }
        }
    });
    Ok(rx)
}

impl DebugCommand {
    pub(crate) fn to_bytes(self) -> [u8; DEBUG_BYTES_PER_COMMAND] {
        let mut result = [0u8; DEBUG_BYTES_PER_COMMAND];
        result[0] = mcu_common::DEBUG_COMMAND_START;
        result[1] = self.code;
        result[2..4].clone_from_slice(&self.arg.to_le_bytes());
        result
    }
}

impl CommandEditor {
    fn get_commands(&mut self) -> Vec<Command> {
        if self.should_exit {
            return vec![Command::LocalOperation(LocalOperation::Exit)];
        }
        while let Some(line) = self.readline(">> ") {
            if line.is_empty() {
                return vec![];
            }
            self.editor.add_history_entry(&line);
            if let Ok(commands) = parse_commands(&line) {
                return commands;
            }
        }
        vec![]
    }

    fn readline(&mut self, prompt: &str) -> Option<String> {
        match self.editor.readline(prompt) {
            Ok(line) => return Some(line),
            // Ctrl-D or Ctrl-C should exit. We exit by setting should_exit
            // which is then checked next time we're going to prompt for a
            // command. We could return something here that indicates to the
            // caller that we should exit, however there are multiple callers
            // and we'd rather not have each of them check that.
            Err(rustyline::error::ReadlineError::Eof)
            | Err(rustyline::error::ReadlineError::Interrupted) => self.should_exit = true,
            Err(_) => {}
        }
        None
    }

    fn wait_for_enter(&mut self) {
        if !self.should_exit {
            self.readline("");
        }
    }
}

fn parse_commands(commands_str: &str) -> Result<Vec<Command>, ()> {
    if commands_str.is_empty() {
        return Ok(Vec::new());
    }
    let mut got_error = false;
    let commands = commands_str
        .split(';')
        .filter_map(|part| match parse_command(part.trim()) {
            Ok(x) => x,
            Err(error) => {
                eprintln!("{}", error);
                got_error = true;
                None
            }
        })
        .collect();
    if got_error {
        Err(())
    } else {
        Ok(commands)
    }
}

fn parse_command(command_str: &str) -> Result<Option<Command>> {
    let mut parts = command_str.split(' ');
    if let Some(keyword) = parts.next() {
        for command in COMMANDS {
            if command.keyword == keyword {
                match command.command_type {
                    CommandType::FpgaCommand(code) => {
                        return Ok(Some(Command::FpgaCommand(DebugCommand { code, arg: 0 })));
                    }
                    CommandType::McuCommand(mcu_command) => {
                        return Ok(Some(Command::McuCommand(DebugCommand {
                            code: mcu_command.into(),
                            arg: 0,
                        })));
                    }
                    CommandType::McuCommandWithArg(mcu_command) => {
                        return Ok(Some(Command::McuCommand(DebugCommand {
                            code: mcu_command.into(),
                            arg: get_arg(command, parts.next())?,
                        })));
                    }
                    CommandType::FpgaCommandWithArg(code) => {
                        return Ok(Some(Command::FpgaCommand(DebugCommand {
                            code,
                            arg: get_arg(command, parts.next())?,
                        })));
                    }
                    CommandType::LocalOperation(op) => {
                        return Ok(Some(Command::LocalOperation(op)));
                    }
                }
            }
        }
        bail!(
            "Unknown command '{}'. Type 'help' to see list of commands",
            keyword
        );
    }
    Ok(None)
}

fn get_arg(command: &AvailableDebugCommand, next_arg: Option<&str>) -> Result<u16> {
    if let Some(arg_string) = next_arg {
        if let Some(arg) = parse_value(arg_string) {
            return Ok(arg);
        } else {
            bail!(
                "The argument '{}' couldn't be parsed as a decimal or hex u16",
                arg_string
            );
        }
    }
    bail!("The command {} requires an argument", command.keyword);
}

pub(crate) fn print_help_text() {
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
        if let Some(prefix) = &line[..pos].split(';').last() {
            let prefix = prefix.trim();
            let mut completions = Vec::new();
            for command in COMMANDS {
                if command.keyword.starts_with(prefix) {
                    completions.push(command.keyword.to_owned());
                }
            }
            let prefix_offset = prefix.as_ptr() as usize - line.as_ptr() as usize;
            Ok((prefix_offset, completions))
        } else {
            Ok((0, vec![]))
        }
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
