// SPDX-License-Identifier: Apache-2.0
//! Command line grammar, compatible with Yepkit's ykurcmd:
//! `[-s SERIAL] -u|-d PORT`, `[-s SERIAL] -g PORT`, `[-s SERIAL] -v`, each
//! optionally with `--no-wait`, before or after `-s`, plus `-l`, `--version` and `-h`. PORT is 1 to 4,
//! `r` for the relay, `a` for the four ports, or `everything` for the four ports and the relay.

use ykur::{Error, Output, Result, Target};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Switch(Target, bool),
    State(Target),
    FirmwareVersion,
    List,
    Version,
    Help,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub serial: Option<String>,
    /// Fail at once instead of waiting for a board that is in use.
    pub no_wait: bool,
    pub command: Command,
}

pub fn parse(args: &[String]) -> Result<Invocation> {
    let mut rest = args.iter().map(String::as_str);
    let mut serial = None;
    let mut no_wait = false;

    let command = loop {
        break match rest.next() {
            Some("--no-wait") if !no_wait => {
                no_wait = true;
                continue;
            }
            Some("-s") if serial.is_none() => {
                let value = rest
                    .next()
                    .ok_or_else(|| usage("-s needs a serial number"))?;
                serial = Some(value.to_owned());
                continue;
            }
            Some(option @ ("--no-wait" | "-s")) => {
                return Err(usage(&format!("{option} given more than once")))
            }
            None => return Err(usage("No command given")),
            Some("-u") => Command::Switch(target(rest.next(), "-u")?, true),
            Some("-d") => Command::Switch(target(rest.next(), "-d")?, false),
            Some("-g") => Command::State(target(rest.next(), "-g")?),
            Some("-v") => Command::FirmwareVersion,
            Some("-l") => Command::List,
            Some("--version") => Command::Version,
            Some("-h" | "--help") => Command::Help,
            Some(other) => return Err(usage(&format!("Unknown option {other}"))),
        };
    };

    if let Some(extra) = rest.next() {
        return Err(usage(&format!("Unexpected argument {extra}")));
    }
    let addresses_board = !matches!(command, Command::List | Command::Version | Command::Help);
    if serial.is_some() && !addresses_board {
        return Err(usage("-s only goes with a command that addresses a board"));
    }
    if no_wait && !addresses_board {
        return Err(usage(
            "--no-wait only goes with a command that addresses a board",
        ));
    }

    Ok(Invocation {
        serial,
        no_wait,
        command,
    })
}

fn target(arg: Option<&str>, option: &str) -> Result<Target> {
    match arg {
        Some("1") => Ok(Target::One(Output::Port1)),
        Some("2") => Ok(Target::One(Output::Port2)),
        Some("3") => Ok(Target::One(Output::Port3)),
        Some("4") => Ok(Target::One(Output::Port4)),
        Some("r") => Ok(Target::One(Output::Relay)),
        // As in Yepkit's ykurcmd (undocumented there): the four ports, not
        // the relay.
        Some("a") => Ok(Target::Ports),
        // Not `all`: ykurcmd looks at the first character only, so there
        // `-d all` leaves the relay alone. A script moved over must not start
        // switching the load on the relay without a word.
        Some("everything") => Ok(Target::All),
        Some(other) => Err(usage(&format!(
            "Invalid port {other}: expected 1, 2, 3, 4, r, a or everything"
        ))),
        None => Err(usage(&format!(
            "{option} needs a port: 1, 2, 3, 4, r, a or everything"
        ))),
    }
}

fn usage(msg: &str) -> Error {
    Error::Usage(msg.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(line: &str) -> Result<Invocation> {
        parse(
            &line
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    }

    fn command(line: &str) -> Command {
        parse_str(line).expect(line).command
    }

    fn usage_error(line: &str) -> String {
        match parse_str(line) {
            Err(Error::Usage(msg)) => msg,
            other => panic!("{line}: expected a usage error, got {other:?}"),
        }
    }

    #[test]
    fn switching_takes_every_port_the_relay_and_everything() {
        assert_eq!(
            command("-u 1"),
            Command::Switch(Target::One(Output::Port1), true)
        );
        assert_eq!(
            command("-d 4"),
            Command::Switch(Target::One(Output::Port4), false)
        );
        assert_eq!(
            command("-u r"),
            Command::Switch(Target::One(Output::Relay), true)
        );
        assert_eq!(command("-d a"), Command::Switch(Target::Ports, false));
        assert_eq!(command("-u everything"), Command::Switch(Target::All, true));
        assert_eq!(
            command("-d everything"),
            Command::Switch(Target::All, false)
        );
    }

    #[test]
    fn the_state_takes_the_same_ports() {
        assert_eq!(command("-g 2"), Command::State(Target::One(Output::Port2)));
        assert_eq!(command("-g r"), Command::State(Target::One(Output::Relay)));
        assert_eq!(command("-g a"), Command::State(Target::Ports));
        assert_eq!(command("-g everything"), Command::State(Target::All));
    }

    #[test]
    fn a_serial_number_selects_the_board() {
        let inv = parse_str("-s YK22170 -u r").unwrap();
        assert_eq!(inv.serial.as_deref(), Some("YK22170"));
        assert_eq!(
            inv.command,
            Command::Switch(Target::One(Output::Relay), true)
        );
    }

    #[test]
    fn no_wait_goes_before_the_command() {
        let inv = parse_str("--no-wait -g r").unwrap();
        assert!(inv.no_wait);
        assert_eq!(inv.command, Command::State(Target::One(Output::Relay)));
        for line in ["--no-wait -s YK1 -u 1", "-s YK1 --no-wait -u 1"] {
            let inv = parse_str(line).unwrap();
            assert!(inv.no_wait, "{line}");
            assert_eq!(inv.serial.as_deref(), Some("YK1"), "{line}");
        }
        assert!(!parse_str("-g r").unwrap().no_wait);
    }

    #[test]
    fn no_wait_with_a_command_that_needs_no_board_is_a_usage_error() {
        usage_error("--no-wait -l");
    }

    #[test]
    fn information_commands() {
        assert_eq!(command("-v"), Command::FirmwareVersion);
        assert_eq!(command("-l"), Command::List);
        assert_eq!(command("--version"), Command::Version);
        assert_eq!(command("-h"), Command::Help);
        assert_eq!(command("--help"), Command::Help);
    }

    #[test]
    fn no_arguments_is_a_usage_error() {
        usage_error("");
    }

    #[test]
    fn a_bad_port_is_named() {
        assert!(usage_error("-u 5").contains("5"));
        assert!(usage_error("-g x").contains("x"));
    }

    #[test]
    fn all_is_refused_since_ykurcmd_takes_it_for_the_four_ports() {
        for line in ["-u all", "-d all", "-g all"] {
            assert!(usage_error(line).starts_with("Invalid port all"), "{line}");
        }
    }

    #[test]
    fn a_missing_port_is_a_usage_error() {
        usage_error("-u");
        usage_error("-s YK1 -g");
    }

    #[test]
    fn an_option_given_twice_is_named_as_such() {
        assert_eq!(
            usage_error("--no-wait --no-wait -g r"),
            "--no-wait given more than once"
        );
        assert_eq!(usage_error("-s A -s B -g r"), "-s given more than once");
    }

    #[test]
    fn a_missing_serial_is_a_usage_error() {
        usage_error("-s");
    }

    #[test]
    fn an_unknown_option_is_named() {
        assert!(usage_error("-x").contains("-x"));
    }

    #[test]
    fn extra_arguments_are_a_usage_error() {
        usage_error("-u 1 2");
        usage_error("-v -l");
    }

    #[test]
    fn a_serial_with_a_command_that_needs_no_board_is_a_usage_error() {
        usage_error("-s YK1 -l");
    }
}
