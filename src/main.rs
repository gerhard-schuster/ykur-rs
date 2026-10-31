// SPDX-License-Identifier: Apache-2.0
//! `ykurctrl`: switch the relay and ports of a Yepkit YKUR and read their state.

mod cli;

use std::io::{self, Write};
use std::process::ExitCode;

use cli::Command;
use ykur::{Error, Result, Transport, Ykur};

const USAGE: &str = "\
Usage: ykurctrl [-s SERIAL] COMMAND

  -u PORT      switch on                PORT = 1|2|3|4|r|a
  -d PORT      switch off               PORT = 1|2|3|4|r|a
  -g PORT      print whether it is on   PORT = 1|2|3|4|r|a
  -v           print the firmware version of the board
  -l           list the attached boards
  -s SERIAL    address the board with this serial number
  --version    print the version of ykurctrl
  -h, --help   print this help

PORT: 1 to 4 are the relay driver ports, r the on-board relay, a all four ports.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| ykur::sanitize(&arg.to_string_lossy()))
        .collect();
    let mut out = io::stdout().lock();

    match run(&args, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Usage(msg)) => {
            let _ = writeln!(io::stderr(), "{msg}\n\n{USAGE}");
            ExitCode::FAILURE
        }
        Err(err) => {
            let _ = writeln!(io::stderr(), "{err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String], out: &mut impl Write) -> Result<()> {
    let invocation = cli::parse(args)?;
    match &invocation.command {
        Command::Help => write!(out, "{USAGE}")?,
        Command::Version => writeln!(out, "ykurctrl {}", env!("CARGO_PKG_VERSION"))?,
        Command::List => print_boards(out, &ykur::list()?)?,
        command => {
            let board = Ykur::open(invocation.serial.as_deref())?;
            execute(&board, command, out)?;
        }
    }
    Ok(())
}

fn execute<T: Transport>(board: &Ykur<T>, command: &Command, out: &mut impl Write) -> Result<()> {
    match command {
        Command::Switch(target, on) => board.switch(*target, *on)?,
        Command::State(target) => {
            for output in target.outputs() {
                let on = board.state(output)?;
                writeln!(out, "{output}: {}", if on { "on" } else { "off" })?;
            }
        }
        Command::FirmwareVersion => writeln!(out, "Firmware {}", board.firmware_version()?)?,
        Command::List | Command::Version | Command::Help => {
            unreachable!("command does not address a board")
        }
    }
    Ok(())
}

fn print_boards(out: &mut impl Write, serials: &[String]) -> Result<()> {
    writeln!(out, "YKUR boards on this host:")?;
    if serials.is_empty() {
        writeln!(out, "  none")?;
    }
    for (i, serial) in serials.iter().enumerate() {
        writeln!(out, "  {:>2}  {serial}", i + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ykur::fake::FakeBoard;
    use ykur::{Output, Target};

    fn output_of(fake: FakeBoard, command: Command) -> (Result<()>, String) {
        let board = Ykur::new(fake);
        let mut out = Vec::new();
        let result = execute(&board, &command, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn the_state_of_one_output_is_one_line() {
        let (result, out) = output_of(
            FakeBoard::new().then(&[0x03, 0x11, 0x01]),
            Command::State(Target::One(Output::Relay)),
        );
        result.unwrap();
        assert_eq!(out, "Relay: on\n");
    }

    #[test]
    fn the_state_of_all_outputs_lists_each() {
        let fake = [0x01, 0x02, 0x03, 0x04]
            .iter()
            .fold(FakeBoard::new(), |f, &code| f.then(&[0x03, code, 0x00]));
        let (result, out) = output_of(fake, Command::State(Target::Ports));
        result.unwrap();
        assert_eq!(out, "Port 1: off\nPort 2: off\nPort 3: off\nPort 4: off\n");
    }

    #[test]
    fn the_firmware_version_is_printed() {
        let (result, out) = output_of(
            FakeBoard::new().then(&[0x20, 0x01, 1, 2, 1]),
            Command::FirmwareVersion,
        );
        result.unwrap();
        assert_eq!(out, "Firmware 1.2.1\n");
    }

    #[test]
    fn switching_prints_nothing() {
        let (result, out) = output_of(
            FakeBoard::new()
                .then(&[0x01, 0x01, 0x00])
                .then(&[0x03, 0x01, 0x01]),
            Command::Switch(Target::One(Output::Port(1)), true),
        );
        result.unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn the_board_list_numbers_each_board() {
        let mut out = Vec::new();
        print_boards(&mut out, &["YK22170".into(), "YK1".into()]).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "YKUR boards on this host:\n   1  YK22170\n   2  YK1\n"
        );
    }

    #[test]
    fn an_empty_board_list_says_none() {
        let mut out = Vec::new();
        print_boards(&mut out, &[]).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "YKUR boards on this host:\n  none\n"
        );
    }
}
