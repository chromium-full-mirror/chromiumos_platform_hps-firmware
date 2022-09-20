// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use mcu_common::commands;
use mcu_common::registers::Register;
use mcu_common::McuDebugCommand;
use mcu_common::DEBUG_BYTES_PER_COMMAND;
use rustyline::config::Configurer;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone, Copy)]
pub(crate) struct DebugCommand {
    pub(crate) code: u8,
    pub(crate) arg: u16,
}

struct AvailableDebugCommand {
    keyword: &'static str,
    help_text: &'static str,
    command_type: CommandType,
    arg_parser: &'static dyn ArgumentParser,
}

trait ArgumentParser {
    /// Converts the input string into a u16 to be sent to the device.
    fn as_u16_for_command(&self, command: &AvailableDebugCommand, arg: Option<&str>)
        -> Result<u16>;

    fn help_text(&self) -> &'static str;
}

struct U16Argument {}
struct I8Argument {}
struct NoArgument {}

const U16_ARG: &dyn ArgumentParser = &U16Argument {};
const I8_ARG: &dyn ArgumentParser = &I8Argument {};
const NO_ARG: &dyn ArgumentParser = &NoArgument {};

enum CommandType {
    FpgaCommand(fpga_app::DebugCommand),
    McuCommand(McuDebugCommand),
    LocalOperation(LocalOperation),
    ExitCommand,
}

#[derive(Clone, Copy)]
pub(crate) enum LocalOperation {
    PrintHelp,
    ResetMcu,
    WriteGateware,
    WriteSocRom,
    WriteTestImages,
    I2cCommand(mcu_common::commands::Command),
    SetFeatureEnable,
    SleepMilliseconds,
    SetI2cRegister(Register),
    WriteMcuProgram,
}

#[derive(Clone, Copy)]
pub(crate) struct LocalCommand {
    pub(crate) operation: LocalOperation,
    pub(crate) arg: u16,
}

pub(crate) enum Command {
    Fpga(DebugCommand),
    Mcu(DebugCommand),
    Local(LocalCommand),
    Exit,
}

struct CommandEditor {
    editor: rustyline::Editor<CompletionHelper>,
    should_exit: bool,
}

#[derive(Clone)]
pub(crate) struct ConsoleOutput {
    external_printer: Arc<Mutex<dyn rustyline::ExternalPrinter>>,
}

impl ConsoleOutput {
    pub(crate) fn print<T: Into<String>>(&self, out: T) {
        // This will probably only fail if stdout gets closed. If that happens,
        // then we don't have any useful way to report the error, so we ignore
        // it.
        let _ = self.external_printer.lock().unwrap().print(out.into());
    }
}

#[derive(Default)]
struct CompletionHelper {}

const COMMANDS: &[AvailableDebugCommand] = &[
    // TODO: Consider changing commands that set things on and off to single
    // commands with an argument. That would require changing the FPGA side as
    // well, so isn't done initially.
    AvailableDebugCommand {
        keyword: "transfer",
        help_text: "Enable image transfer. 1=on, 0=off",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::Transfer),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "transfer_count",
        help_text: "Transfer a given number of images",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::TransferCount),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "histogram",
        help_text: "Show histogram of image values. 1=on, 0=off",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::Histogram),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "fpga_self_test",
        help_text: "Run FPGA self test",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::SelfTest),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_camera_config",
        help_text: "Set the camera configuration bits. e.g. 0=no rotation, 1=rotate clockwise",
        command_type: CommandType::LocalOperation(LocalOperation::SetI2cRegister(
            Register::CameraConfig,
        )),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_feature_enable",
        help_text: "Set contents of feature enable register. 0=off, 1=presence, 2=SPD, 3=both",
        command_type: CommandType::LocalOperation(LocalOperation::SetFeatureEnable),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_fpga_exposure",
        help_text: "Sets camera exposure from FPGA. 0=minimum 0x1fff=maximum",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::SetExposure),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_exposure_median_target",
        help_text: "Sets camera exposure from from FPGA. Range -127 to 128",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::SetMedianTarget),
        arg_parser: I8_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_hardware_ae",
        help_text: "Enables hardware automatic exposure with the specified target",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::HardwareAe),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "disable_automatic_exposure",
        help_text: "Disables all auto-exposure so that camera settings can be set manually",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::DisableAutomaticExposure),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "launch_app",
        help_text: "Launch application (start FPGA)",
        command_type: CommandType::LocalOperation(LocalOperation::I2cCommand(
            commands::Command::LaunchApp,
        )),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "launch_app_without_i2c",
        help_text: "Launch application (start FPGA) - without using I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::TryStartFpga),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "write_spi_flash_test_data",
        help_text: "Write SPI flash test data",
        command_type: CommandType::LocalOperation(LocalOperation::I2cCommand(
            commands::Command::WriteSpiFlashTestData,
        )),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "test_fpga_spi_flash_reads",
        help_text: "Read last 1MB of SPI flash, checking for expected data",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::TestSpiFlashReads),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "test_fpga_mcu_communication",
        help_text: "Check communication between the FPGA and the MCU",
        command_type: CommandType::FpgaCommand(fpga_app::DebugCommand::TestFpgaMcuComms),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "help",
        help_text: "Print command help",
        command_type: CommandType::LocalOperation(LocalOperation::PrintHelp),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "exit",
        help_text: "Exit from hps-mon",
        command_type: CommandType::ExitCommand,
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "reset_fpga",
        help_text: "Reset the FPGA",
        command_type: CommandType::McuCommand(McuDebugCommand::ResetFpga),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "read_camera_register",
        help_text: "Reads camera register over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadCameraRegister),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "read_camera_id",
        help_text: "Reads camera ID over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadCameraId),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "read_camera_config",
        help_text: "Reads camera config over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadCameraConfig),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_camera_blc_target",
        help_text: "Sets camera black level control targets",
        command_type: CommandType::McuCommand(McuDebugCommand::SetCameraBlcTarget),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_camera_digital_gain",
        help_text: "Sets camera digital gain over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::SetCameraDigitalGain),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_camera_analog_gain",
        help_text: "Sets camera analog gain over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::SetCameraAnalogGain),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_camera_integration",
        help_text: "Sets camera integration over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::SetCameraIntegration),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_camera_ae_target",
        help_text: "Sets camera AE target over I2C",
        command_type: CommandType::McuCommand(McuDebugCommand::SetCameraAeTarget),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "reset_mcu",
        help_text: "Reset the MCU",
        command_type: CommandType::LocalOperation(LocalOperation::ResetMcu),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_blocking_mode",
        help_text: "Sets FPGA to host RTT channel as 1=blocking, 0=non-blocking",
        command_type: CommandType::McuCommand(McuDebugCommand::SetBlockingMode),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "sleep",
        help_text: "Sleep for N milliseconds",
        command_type: CommandType::LocalOperation(LocalOperation::SleepMilliseconds),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "ping_mcu",
        help_text: "Ask the MCU to print something",
        command_type: CommandType::McuCommand(McuDebugCommand::Ping),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "mcu_panic",
        help_text: "Trigger a panic on the MCU",
        command_type: CommandType::LocalOperation(LocalOperation::I2cCommand(
            mcu_common::commands::Command::TriggerMcuPanic,
        )),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "read_spi_flash",
        help_text: "Ask the MCU to read part of the spi flash and print it",
        command_type: CommandType::McuCommand(McuDebugCommand::ReadSpiFlash),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "write_gateware",
        help_text: "Write gateware to SPI flash",
        command_type: CommandType::LocalOperation(LocalOperation::WriteGateware),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "write_soc_rom",
        help_text: "Write soft CPU binary to SPI flash",
        command_type: CommandType::LocalOperation(LocalOperation::WriteSocRom),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "write_test_images",
        help_text: "Write all PNG files in --test-image-dir to test data area on the SPI flash",
        command_type: CommandType::LocalOperation(LocalOperation::WriteTestImages),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "erase_spi_flash",
        help_text: "Erase entire SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::EraseSpiFlash),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "hash_spi_flash",
        help_text: "Hash part of the SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::HashSpiFlash),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "spi_flash_read_speed",
        help_text: "Read 1MB of SPI flash and report speed",
        command_type: CommandType::McuCommand(McuDebugCommand::SpiFlashReadSpeed),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "spi_flash_write_speed",
        help_text: "Write 1MB of SPI flash and report speed",
        command_type: CommandType::McuCommand(McuDebugCommand::SpiFlashWriteSpeed),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "fpga_power_off",
        help_text: "Power off the FPGA and SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::FpgaPowerOff),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "fpga_power_on",
        help_text: "Power on the FPGA and SPI flash",
        command_type: CommandType::McuCommand(McuDebugCommand::FpgaPowerOn),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "mlb_interrupt",
        help_text: "Pulse interrupt signal to MLB",
        command_type: CommandType::LocalOperation(LocalOperation::I2cCommand(
            commands::Command::MlbInterrupt,
        )),
        arg_parser: NO_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_debug1",
        help_text: "Set debug register 1",
        command_type: CommandType::LocalOperation(LocalOperation::SetI2cRegister(Register::Debug1)),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_debug2",
        help_text: "Set debug register 2",
        command_type: CommandType::LocalOperation(LocalOperation::SetI2cRegister(Register::Debug2)),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "set_debug3",
        help_text: "Set debug register 3",
        command_type: CommandType::LocalOperation(LocalOperation::SetI2cRegister(Register::Debug3)),
        arg_parser: U16_ARG,
    },
    AvailableDebugCommand {
        keyword: "write_mcu",
        help_text: "Writes the MCU program",
        command_type: CommandType::LocalOperation(LocalOperation::WriteMcuProgram),
        arg_parser: NO_ARG,
    },
];

/// Sets up console input and output. Returned values are a source of commands
/// and a way to send output to the console. Terminates when the returned
/// command receiver gets closed.
pub(crate) fn create_console(
    initial_commands: &str,
) -> Result<(mpsc::Receiver<Command>, ConsoleOutput)> {
    let mut editor = CommandEditor {
        editor: rustyline::Editor::<CompletionHelper>::new()?,
        should_exit: false,
    };
    editor.editor.set_helper(Some(CompletionHelper::default()));
    editor
        .editor
        .set_completion_type(rustyline::CompletionType::List);
    let (tx, rx) = mpsc::channel();
    let console_output = ConsoleOutput {
        external_printer: Arc::new(Mutex::new(editor.editor.create_external_printer()?)),
    };
    let outputs = (rx, console_output);
    if let Ok(commands) = parse_commands(initial_commands) {
        for command in commands {
            let is_exit = matches!(command, Command::Exit);
            if tx.send(command).is_err() || is_exit {
                return Ok(outputs);
            }
        }
    }
    std::thread::spawn(move || {
        println!("Welcome to hps-mon.");
        loop {
            let commands = editor.get_commands();
            for command in commands {
                let is_exit = matches!(command, Command::Exit);
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
    Ok(outputs)
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
            return vec![Command::Exit];
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
                let arg = command
                    .arg_parser
                    .as_u16_for_command(command, parts.next())?;
                match command.command_type {
                    CommandType::FpgaCommand(code) => {
                        return Ok(Some(Command::Fpga(DebugCommand {
                            code: code.into(),
                            arg,
                        })));
                    }
                    CommandType::McuCommand(mcu_command) => {
                        return Ok(Some(Command::Mcu(DebugCommand {
                            code: mcu_command.into(),
                            arg,
                        })));
                    }
                    CommandType::LocalOperation(op) => {
                        return Ok(Some(Command::Local(LocalCommand { operation: op, arg })));
                    }
                    CommandType::ExitCommand => return Ok(Some(Command::Exit)),
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

impl ArgumentParser for U16Argument {
    fn as_u16_for_command(
        &self,
        command: &AvailableDebugCommand,
        arg: Option<&str>,
    ) -> Result<u16> {
        let string =
            arg.ok_or_else(|| anyhow!("The command {} requires an argument", command.keyword))?;
        parse_value(string).ok_or_else(|| {
            anyhow!(
                "The argument '{}' couldn't be parsed as a decimal or hex u16",
                string
            )
        })
    }

    fn help_text(&self) -> &'static str {
        "{u16}"
    }
}

impl ArgumentParser for I8Argument {
    fn as_u16_for_command(
        &self,
        command: &AvailableDebugCommand,
        arg: Option<&str>,
    ) -> Result<u16> {
        let string =
            arg.ok_or_else(|| anyhow!("The command {} requires an argument", command.keyword))?;
        string
            .parse::<i8>()
            .map(|i8_value| i8_value as u16)
            .map_err(|_| anyhow!("The argument '{}' couldn't be parsed as an i8", string))
    }

    fn help_text(&self) -> &'static str {
        "{u16}"
    }
}

impl ArgumentParser for NoArgument {
    fn as_u16_for_command(
        &self,
        command: &AvailableDebugCommand,
        arg: Option<&str>,
    ) -> Result<u16> {
        if arg.is_some() {
            bail!("The command {} doesn't take an argument", command.keyword);
        }
        Ok(0)
    }

    fn help_text(&self) -> &'static str {
        ""
    }
}

pub(crate) fn print_help_text(console_output: &ConsoleOutput) {
    for command in COMMANDS {
        console_output.print(format!(
            "{:30} {}\n",
            format!("{} {}", command.keyword, command.arg_parser.help_text()),
            command.help_text
        ));
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
