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
    /// The HID stack refused the operation. Carries the message only, so the
    /// error type of the HID library does not become part of this interface.
    Hid(String),
    /// The board did not answer within the read timeout.
    NoResponse,
    /// Another invocation had the board, and it did not come free in time.
    Busy {
        /// The board, by its serial number.
        serial: String,
        /// How long was waited; zero when waiting was not asked for.
        waited: std::time::Duration,
        /// Who has the board, as far as can be told.
        cause: String,
    },
    /// The board answered, but reported a failure or an unexpected report.
    Device(String),
    /// Writing the output failed.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(msg) | Error::Device(msg) => write!(f, "{msg}"),
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

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<async_hid::HidError> for Error {
    fn from(e: async_hid::HidError) -> Self {
        Error::Hid(e.to_string())
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
