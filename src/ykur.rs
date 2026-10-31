// SPDX-License-Identifier: Apache-2.0
//! The YKUR control protocol.
//!
//! Source: YKUR datasheet rev. 1.2.1, section 4. Where firmware 1.2.1 answers
//! differently from that document, the measured answer is accepted too; see the
//! README, "Where the board differs from the datasheet".

use std::fmt;

use crate::device::{report, Board, Report, Transport};
use crate::error::{Error, Result};

const SWITCH_ON: u8 = 0x01;
const SWITCH_OFF: u8 = 0x02;
const GET_STATE: u8 = 0x03;
const GET_VERSION: [u8; 2] = [0x20, 0x01];

/// Status byte of a failed command (datasheet 4.1.1, 4.1.2).
const STATUS_ERROR: u8 = 0xAA;
/// First byte of an answer that reports an unknown error (datasheet 4.1).
const UNKNOWN_ERROR: u8 = 0x00;

/// One switchable output of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// One of the four relay driver ports, 1 to 4.
    Port(u8),
    /// The on-board relay.
    Relay,
}

impl Output {
    /// Every output, in the order they are reported.
    pub const ALL: [Output; 5] = [
        Output::Port(1),
        Output::Port(2),
        Output::Port(3),
        Output::Port(4),
        Output::Relay,
    ];

    fn code(self) -> u8 {
        match self {
            Output::Port(n) => n,
            Output::Relay => 0x11,
        }
    }
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Port(n) => write!(f, "Port {n}"),
            Output::Relay => write!(f, "Relay"),
        }
    }
}

/// What a switching command acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    One(Output),
    /// The four relay driver ports at once.
    Ports,
    /// The four ports and the relay at once.
    All,
}

impl Target {
    fn code(self) -> u8 {
        match self {
            Target::One(output) => output.code(),
            Target::Ports => 0x0A,
            Target::All => 0xAA,
        }
    }

    /// The outputs a target covers.
    pub fn outputs(self) -> Vec<Output> {
        match self {
            Target::One(output) => vec![output],
            Target::Ports => Output::ALL[..4].to_vec(),
            Target::All => Output::ALL.to_vec(),
        }
    }
}

/// A three part firmware version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A YKUR board, reached through a [`Transport`].
pub struct Ykur<T: Transport = Board> {
    transport: T,
}

impl Ykur<Board> {
    /// Opens the board with the given serial number, or the first one found.
    pub fn open(serial: Option<&str>) -> Result<Self> {
        Ok(Ykur::new(Board::open(serial)?))
    }

    /// Like [`Ykur::open`], waiting at most `wait` for a board another
    /// invocation is using; see [`Board::open_within`].
    pub fn open_within(serial: Option<&str>, wait: std::time::Duration) -> Result<Self> {
        Ok(Ykur::new(Board::open_within(serial, wait)?))
    }
}

impl<T: Transport> Ykur<T> {
    pub fn new(transport: T) -> Self {
        Ykur { transport }
    }

    /// The transport, for inspecting a test double.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Switches a target on or off and confirms the new state by reading it
    /// back.
    pub fn switch(&self, target: Target, on: bool) -> Result<()> {
        let action = if on { SWITCH_ON } else { SWITCH_OFF };
        let answer = self.exchange(&[action, target.code()])?;

        // The datasheet echoes the action; firmware 1.2.1 answers a switch off
        // with the state command code instead (measured, see the README).
        let echoed = answer[0] == action || (!on && answer[0] == GET_STATE);
        if !echoed || answer[1] != target.code() {
            return Err(unexpected("switching", &answer));
        }
        if answer[2] == STATUS_ERROR {
            return Err(Error::Device(format!(
                "The board reported an error switching {}",
                describe(target)
            )));
        }

        // The status byte of the answer is not what the datasheet says on
        // firmware 1.2.1, so the outcome is read back instead.
        for output in target.outputs() {
            if self.state(output)? != on {
                return Err(Error::Device(format!(
                    "{output} did not switch {}",
                    if on { "on" } else { "off" }
                )));
            }
        }
        Ok(())
    }

    /// Whether an output is on.
    pub fn state(&self, output: Output) -> Result<bool> {
        let answer = self.exchange(&[GET_STATE, output.code()])?;
        if answer[0] != GET_STATE || answer[1] != output.code() {
            return Err(unexpected("the state", &answer));
        }
        match answer[2] {
            0x00 => Ok(false),
            0x01 => Ok(true),
            STATUS_ERROR => Err(Error::Device(format!(
                "The board reported an error reading the state of {output}"
            ))),
            _ => Err(unexpected("the state", &answer)),
        }
    }

    /// The firmware version of the board.
    pub fn firmware_version(&self) -> Result<Version> {
        let answer = self.exchange(&GET_VERSION)?;
        if answer[..2] != GET_VERSION {
            return Err(unexpected("the firmware version", &answer));
        }
        Ok(Version {
            major: answer[2],
            minor: answer[3],
            patch: answer[4],
        })
    }

    fn exchange(&self, payload: &[u8]) -> Result<Report> {
        let answer = self.transport.transfer(&report(payload))?;
        if answer[0] == UNKNOWN_ERROR {
            return Err(Error::Device("The board reported an unknown error".into()));
        }
        Ok(answer)
    }
}

fn describe(target: Target) -> String {
    match target {
        Target::One(output) => output.to_string(),
        Target::Ports => "the four ports".into(),
        Target::All => "all outputs".into(),
    }
}

fn unexpected(what: &str, answer: &Report) -> Error {
    Error::Device(format!(
        "Unexpected answer to {what}: {:02x} {:02x} {:02x}",
        answer[0], answer[1], answer[2]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeBoard;

    fn board(fake: FakeBoard) -> Ykur<FakeBoard> {
        Ykur::new(fake)
    }

    fn device_error<V: fmt::Debug>(result: Result<V>) -> String {
        match result {
            Err(Error::Device(msg)) => msg,
            other => panic!("expected a device error, got {other:?}"),
        }
    }

    // Firmware version (datasheet 4.2.1)

    #[test]
    fn the_firmware_version_is_read_from_bytes_three_to_five() {
        let b = board(FakeBoard::new().then(&[0x20, 0x01, 1, 2, 1]));
        assert_eq!(b.firmware_version().unwrap().to_string(), "1.2.1");
        assert_eq!(b.transport().sent(2), [vec![0x20, 0x01]]);
    }

    #[test]
    fn a_version_answer_to_another_command_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x03, 0x01, 0x00]));
        device_error(b.firmware_version());
    }

    #[test]
    fn an_unknown_error_answer_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x00]));
        assert!(device_error(b.firmware_version()).contains("unknown error"));
    }

    #[test]
    fn a_transport_error_reaches_the_caller() {
        let b = board(FakeBoard::new().then_fail(Error::NoResponse));
        assert!(matches!(b.firmware_version(), Err(Error::NoResponse)));
    }

    // State (datasheet 4.1.2)

    #[test]
    fn the_state_command_names_the_output() {
        for (output, code) in [
            (Output::Port(1), 0x01),
            (Output::Port(2), 0x02),
            (Output::Port(3), 0x03),
            (Output::Port(4), 0x04),
            (Output::Relay, 0x11),
        ] {
            let b = board(FakeBoard::new().then(&[0x03, code, 0x01]));
            assert!(b.state(output).unwrap(), "{output}");
            assert_eq!(b.transport().sent(2), [vec![0x03, code]]);
        }
    }

    #[test]
    fn a_state_of_zero_is_off() {
        let b = board(FakeBoard::new().then(&[0x03, 0x11, 0x00]));
        assert!(!b.state(Output::Relay).unwrap());
    }

    #[test]
    fn a_state_the_board_reports_as_error_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x03, 0x02, 0xAA]));
        device_error(b.state(Output::Port(2)));
    }

    #[test]
    fn a_state_for_another_output_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x03, 0x02, 0x01]));
        device_error(b.state(Output::Port(1)));
    }

    #[test]
    fn an_undocumented_state_value_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x03, 0x01, 0x02]));
        device_error(b.state(Output::Port(1)));
    }

    // Switching (datasheet 4.1.1, and the answers of firmware 1.2.1)

    #[test]
    fn switching_on_as_firmware_1_2_1_answers_is_confirmed_by_the_state() {
        let b = board(
            FakeBoard::new()
                .then(&[0x01, 0x03, 0x00])
                .then(&[0x03, 0x03, 0x01]),
        );
        b.switch(Target::One(Output::Port(3)), true).unwrap();
        assert_eq!(b.transport().sent(2), [vec![0x01, 0x03], vec![0x03, 0x03]]);
    }

    #[test]
    fn switching_off_as_firmware_1_2_1_answers_is_accepted() {
        // Firmware 1.2.1 answers a switch off in the format of a state answer.
        let b = board(
            FakeBoard::new()
                .then(&[0x03, 0x11, 0x00])
                .then(&[0x03, 0x11, 0x00]),
        );
        b.switch(Target::One(Output::Relay), false).unwrap();
        assert_eq!(b.transport().sent(2), [vec![0x02, 0x11], vec![0x03, 0x11]]);
    }

    #[test]
    fn switching_as_the_datasheet_answers_is_accepted() {
        let on = board(
            FakeBoard::new()
                .then(&[0x01, 0x01, 0xFF])
                .then(&[0x03, 0x01, 0x01]),
        );
        on.switch(Target::One(Output::Port(1)), true).unwrap();
        let off = board(
            FakeBoard::new()
                .then(&[0x02, 0x01, 0xFF])
                .then(&[0x03, 0x01, 0x00]),
        );
        off.switch(Target::One(Output::Port(1)), false).unwrap();
    }

    #[test]
    fn a_switch_the_board_reports_as_error_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x01, 0x04, 0xAA]));
        device_error(b.switch(Target::One(Output::Port(4)), true));
        assert!(b.transport().exhausted());
    }

    #[test]
    fn a_switch_answer_for_another_target_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x01, 0x02, 0x00]));
        device_error(b.switch(Target::One(Output::Port(1)), true));
    }

    #[test]
    fn a_switch_answer_to_another_command_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x20, 0x01, 0x00]));
        device_error(b.switch(Target::One(Output::Port(1)), true));
    }

    #[test]
    fn a_switch_answered_with_an_unknown_error_is_an_error() {
        let b = board(FakeBoard::new().then(&[0x00]));
        device_error(b.switch(Target::One(Output::Relay), false));
    }

    #[test]
    fn a_switch_the_state_does_not_confirm_is_an_error() {
        let b = board(
            FakeBoard::new()
                .then(&[0x01, 0x11, 0x00])
                .then(&[0x03, 0x11, 0x00]),
        );
        assert!(device_error(b.switch(Target::One(Output::Relay), true)).contains("Relay"));
    }

    #[test]
    fn switching_everything_checks_every_output() {
        let mut fake = FakeBoard::new().then(&[0x01, 0xAA, 0x00]);
        for output in Output::ALL {
            fake = fake.then(&[0x03, output.code(), 0x01]);
        }
        let b = board(fake);
        b.switch(Target::All, true).unwrap();
        let sent = b.transport().sent(2);
        assert_eq!(sent[0], vec![0x01, 0xAA]);
        assert_eq!(sent.len(), 6);
    }

    #[test]
    fn switching_everything_off_as_firmware_1_2_1_answers_is_accepted() {
        let mut fake = FakeBoard::new().then(&[0x03, 0xAA, 0x02]);
        for output in Output::ALL {
            fake = fake.then(&[0x03, output.code(), 0x00]);
        }
        board(fake).switch(Target::All, false).unwrap();
    }

    #[test]
    fn switching_the_four_ports_checks_the_ports_only() {
        let mut fake = FakeBoard::new().then(&[0x01, 0x0A, 0x00]);
        for port in 1..=4 {
            fake = fake.then(&[0x03, port, 0x01]);
        }
        let b = board(fake);
        b.switch(Target::Ports, true).unwrap();
        let sent = b.transport().sent(2);
        assert_eq!(sent[0], vec![0x01, 0x0A]);
        assert_eq!(sent.len(), 5);
        assert!(b.transport().exhausted());
    }

    #[test]
    fn an_output_left_behind_by_switching_everything_is_named() {
        let mut fake = FakeBoard::new().then(&[0x01, 0xAA, 0x00]);
        for output in Output::ALL {
            let on = output != Output::Port(3);
            fake = fake.then(&[0x03, output.code(), u8::from(on)]);
        }
        assert!(device_error(board(fake).switch(Target::All, true)).contains("Port 3"));
    }

    /// Needs a YKUR attached and nothing on its outputs that must not switch.
    /// Run with `cargo test -- --ignored --test-threads=1`. Each test puts the
    /// outputs back the way it found them.
    mod hardware {
        use super::*;

        fn snapshot(board: &Ykur) -> Vec<bool> {
            Output::ALL
                .iter()
                .map(|&o| board.state(o).expect("state should be readable"))
                .collect()
        }

        fn restore(board: &Ykur, states: &[bool]) {
            for (&output, &on) in Output::ALL.iter().zip(states) {
                board.switch(Target::One(output), on).expect("restore");
            }
        }

        #[test]
        #[ignore = "needs a YKUR attached"]
        fn the_firmware_version_is_answered() {
            let board = Ykur::open(None).expect("a board should open");
            let version = board.firmware_version().expect("version");
            assert!(version.major > 0 || version.minor > 0, "{version}");
        }

        #[test]
        #[ignore = "needs a YKUR attached"]
        fn every_output_switches_on_and_off() {
            let board = Ykur::open(None).expect("a board should open");
            let before = snapshot(&board);
            for output in Output::ALL {
                board.switch(Target::One(output), true).expect("on");
                board.switch(Target::One(output), false).expect("off");
            }
            restore(&board, &before);
        }

        #[test]
        #[ignore = "needs a YKUR attached"]
        fn the_four_ports_switch_at_once_and_leave_the_relay() {
            let board = Ykur::open(None).expect("a board should open");
            let before = snapshot(&board);
            board
                .switch(Target::One(Output::Relay), false)
                .expect("relay off");
            board.switch(Target::Ports, true).expect("ports on");
            assert_eq!(snapshot(&board), [true, true, true, true, false]);
            board.switch(Target::Ports, false).expect("ports off");
            restore(&board, &before);
        }

        #[test]
        #[ignore = "needs a YKUR attached"]
        fn all_outputs_switch_at_once() {
            let board = Ykur::open(None).expect("a board should open");
            let before = snapshot(&board);
            board.switch(Target::All, true).expect("all on");
            assert!(snapshot(&board).iter().all(|&on| on));
            board.switch(Target::All, false).expect("all off");
            assert!(snapshot(&board).iter().all(|&on| !on));
            restore(&board, &before);
        }
    }
}
