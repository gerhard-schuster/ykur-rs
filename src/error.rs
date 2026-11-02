// SPDX-License-Identifier: Apache-2.0
//! Error type shared by the command line, transport and protocol layers.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Not exhaustive: a new way to fail is not a breaking change for a caller
/// that already handles the ones it knows.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The command line could not be interpreted.
    Usage(String),
    /// No board, or no board with the requested serial number, is attached.
    NotFound { serial: Option<String> },
    /// The board, or the HID stack, cannot be reached: the board is no longer
    /// connected, it has no hidraw node, its node keeps changing, or the HID
    /// stack cannot list its devices.
    Unavailable(String),
    /// The system denied access to the board; on Linux the udev rule is
    /// missing or the user is not in its group.
    PermissionDenied,
    /// The HID stack refused the operation. Carries the message only, so the
    /// error type of the HID library does not become part of this interface.
    Hid(String),
    /// The board did not answer within the read timeout.
    NoResponse,
    /// Another invocation or, on macOS, another program had the board, and
    /// it did not come free in time.
    Busy {
        /// The board, by its serial number.
        serial: String,
        /// How long was waited; zero when waiting was not asked for.
        waited: std::time::Duration,
        /// Who has the board, as far as can be told.
        cause: String,
    },
    /// The board reported an error: status `AA`, an answer starting with
    /// `00` (unknown error), or an output that did not switch.
    Rejected(String),
    /// The board's answer does not fit the request: wrong echo, an
    /// undocumented value. Usually an answer meant for another exchange.
    Device(String),
    /// Writing the output failed.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(msg)
            | Error::Device(msg)
            | Error::Rejected(msg)
            | Error::Unavailable(msg) => write!(f, "{msg}"),
            Error::PermissionDenied => write!(f, "{}", crate::device::PERMISSION_DENIED),
            Error::NotFound { serial: Some(s) } => {
                write!(f, "No YKUR board with serial number {s} found")
            }
            Error::NotFound { serial: None } => write!(f, "No YKUR board found"),
            Error::Hid(e) => write!(f, "USB HID error: {e}"),
            Error::NoResponse => write!(f, "No response from the board"),
            Error::Busy {
                serial,
                waited,
                cause,
            } if waited.is_zero() => write!(f, "The YKUR {serial} is not available: {cause}"),
            Error::Busy {
                serial,
                waited,
                cause,
            } => write!(
                f,
                "The YKUR {serial} is still not available after {:.1} s: {cause}",
                waited.as_secs_f64()
            ),
            Error::Io(e) => write!(f, "Output error: {e}"),
        }
    }
}

impl Error {
    /// The exit status for this error, after sysexits(3), chosen by what
    /// the caller should do next:
    ///
    /// | Code | Error | What to do |
    /// |---|---|---|
    /// | 64 `EX_USAGE` | [`Error::Usage`] | correct the call |
    /// | 69 `EX_UNAVAILABLE` | [`Error::NotFound`], [`Error::Unavailable`], [`Error::Rejected`] | stop, check the board and its connection |
    /// | 74 `EX_IOERR` | [`Error::Hid`], [`Error::NoResponse`], [`Error::Io`] | retry if the command may be repeated |
    /// | 75 `EX_TEMPFAIL` | [`Error::Busy`] | retry later |
    /// | 76 `EX_PROTOCOL` | [`Error::Device`] | retry if the command may be repeated; if it recurs, check the board |
    /// | 77 `EX_NOPERM` | [`Error::PermissionDenied`] | fix the setup |
    ///
    /// sysexits(3) has no code for a device that refuses a command; such a
    /// refusal is a valid answer, not a protocol violation, so it gets the
    /// catch-all `EX_UNAVAILABLE`. `EX_NOPERM` is meant for permissions
    /// above the file system; a denied device node is one, but the cause is
    /// the setup, and that is what the code says.
    ///
    /// The match has no catch-all arm, so a new variant does not compile
    /// until it has a code.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage(_) => 64,
            Error::NotFound { .. } | Error::Unavailable(_) | Error::Rejected(_) => 69,
            Error::Hid(_) | Error::NoResponse | Error::Io(_) => 74,
            Error::Busy { .. } => 75,
            Error::Device(_) => 76,
            Error::PermissionDenied => 77,
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<async_hid::HidError> for Error {
    /// A board that is no longer connected is unavailable, not an I/O error
    /// that a retry would cure.
    fn from(e: async_hid::HidError) -> Self {
        match e {
            async_hid::HidError::NotConnected => {
                Error::Unavailable("The YKUR is no longer connected".into())
            }
            e => Error::Hid(e.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_that_is_gone_is_unavailable_not_an_io_error() {
        let error = Error::from(async_hid::HidError::NotConnected);
        assert!(matches!(error, Error::Unavailable(_)), "{error:?}");
        assert_eq!(error.exit_code(), 69);
        assert!(matches!(
            Error::from(async_hid::HidError::message("x")),
            Error::Hid(_)
        ));
    }

    #[test]
    fn every_error_maps_to_the_exit_code_of_what_the_caller_should_do() {
        let busy = Error::Busy {
            serial: "YKTEST".into(),
            waited: std::time::Duration::ZERO,
            cause: "held".into(),
        };
        let cases = [
            (Error::Usage("x".into()), 64),
            (Error::NotFound { serial: None }, 69),
            (Error::Unavailable("x".into()), 69),
            (Error::PermissionDenied, 77),
            (busy, 75),
            (Error::Hid("x".into()), 74),
            (Error::NoResponse, 74),
            (Error::Device("x".into()), 76),
            (Error::Io(std::io::Error::other("x")), 74),
            (Error::Rejected("x".into()), 69),
        ];
        for (error, code) in cases {
            assert_eq!(error.exit_code(), code, "{error:?}");
        }
    }

    #[test]
    fn every_error_says_what_went_wrong() {
        let texts = [
            Error::Usage("bad option".into()).to_string(),
            Error::NotFound {
                serial: Some("YK1".into()),
            }
            .to_string(),
            Error::NotFound { serial: None }.to_string(),
            Error::Hid("refused".into()).to_string(),
            Error::NoResponse.to_string(),
            Error::Device("odd answer".into()).to_string(),
            Error::Io(std::io::Error::other("pipe")).to_string(),
        ];
        assert_eq!(texts[0], "bad option");
        assert_eq!(texts[1], "No YKUR board with serial number YK1 found");
        assert_eq!(texts[2], "No YKUR board found");
        assert_eq!(texts[3], "USB HID error: refused");
        assert_eq!(texts[4], "No response from the board");
        assert_eq!(texts[5], "odd answer");
        assert_eq!(texts[6], "Output error: pipe");
    }

    #[test]
    fn a_board_in_use_names_the_board_the_wait_and_the_cause() {
        let held = |ms| Error::Busy {
            serial: "YK22170".into(),
            waited: std::time::Duration::from_millis(ms),
            cause: "another ykurctrl is using it".into(),
        };
        assert_eq!(
            held(5000).to_string(),
            "The YKUR YK22170 is still not available after 5.0 s: another ykurctrl is using it"
        );
        assert_eq!(
            held(0).to_string(),
            "The YKUR YK22170 is not available: another ykurctrl is using it"
        );
    }

    #[test]
    fn an_output_error_keeps_its_source() {
        use std::error::Error as _;
        assert!(Error::Io(std::io::Error::other("pipe")).source().is_some());
        assert!(Error::NoResponse.source().is_none());
    }
}
