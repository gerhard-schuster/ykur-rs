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
    fn an_output_error_keeps_its_source() {
        use std::error::Error as _;
        assert!(Error::Io(std::io::Error::other("pipe")).source().is_some());
        assert!(Error::NoResponse.source().is_none());
    }
}
