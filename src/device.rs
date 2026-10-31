// SPDX-License-Identifier: Apache-2.0
//! USB HID transport for the YKUR board.
//!
//! Talks to the board through async-hid. That library is asynchronous because
//! that is how macOS delivers HID reports; this program has no reason to be, so
//! the awaits are resolved here and everything above [`Transport`] stays
//! synchronous.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::Duration;

use async_hid::{AsyncHidRead, AsyncHidWrite, Device, DeviceReaderWriter, HidBackend};
use futures_lite::{future, StreamExt};

use crate::error::{Error, Result};
use crate::sanitize::sanitize;

/// USB vendor id of the YKUR (YKUR datasheet rev. 1.2.1, 4).
pub const VENDOR_ID: u16 = 0x04D8;
/// Product id of the YKUR board (YKUR datasheet rev. 1.2.1, 4).
pub const PRODUCT_ID: u16 = 0xF1CB;

/// Every YKUR report is 64 bytes (YKUR datasheet rev. 1.2.1, 4).
pub const REPORT_SIZE: usize = 64;

const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait for a late answer when emptying the queue after a read
/// timed out.
const DISCARD_WAIT: Duration = Duration::from_millis(50);

/// Upper bound on the reports dropped in one go, so a board that streams
/// cannot hold the program in the drain.
const DISCARD_LIMIT: usize = 16;

pub type Report = [u8; REPORT_SIZE];

/// Report exchange with a board. The protocol layer is written against this
/// trait so it can be driven by a test double instead of real hardware.
pub trait Transport {
    /// Sends a report and returns the report the board answers with.
    fn transfer(&self, out: &Report) -> Result<Report>;
}

/// Pads a command payload to a full report.
pub(crate) fn report(payload: &[u8]) -> Report {
    debug_assert!(payload.len() <= REPORT_SIZE);
    let mut buf: Report = [0; REPORT_SIZE];
    buf[..payload.len()].copy_from_slice(payload);
    buf
}

/// An opened YKUR board.
pub struct Board {
    /// Every exchange needs the handle mutably while [`Transport`] hands out a
    /// shared reference; a board is driven from one thread at a time.
    dev: RefCell<DeviceReaderWriter>,

    /// Set when a read timed out. The answer may still arrive afterwards and
    /// would then be queued, so the next exchange empties the queue first.
    stale: Cell<bool>,
}

impl Board {
    /// Opens the board with the given serial number, or the first one found.
    pub fn open(serial: Option<&str>) -> Result<Self> {
        let device = future::block_on(choose(serial))?;
        let dev = future::block_on(device.open()).map_err(open_failed)?;
        Ok(Board {
            dev: RefCell::new(dev),
            stale: Cell::new(false),
        })
    }

    /// Drops reports the board sent after a read had already given up on them.
    /// Errors are swallowed; the exchange that follows raises them properly.
    fn discard_queued(&self) {
        let mut dev = self.dev.borrow_mut();
        let mut scratch: Report = [0; REPORT_SIZE];
        for _ in 0..DISCARD_LIMIT {
            let got = future::block_on(future::or(
                async { Some(dev.read_input_report(&mut scratch).await) },
                async {
                    async_io::Timer::after(DISCARD_WAIT).await;
                    None
                },
            ));
            if !matches!(got, Some(Ok(_))) {
                break;
            }
        }
    }
}

impl Transport for Board {
    fn transfer(&self, out: &Report) -> Result<Report> {
        // An answer to a read that timed out may arrive later and would be
        // taken for the answer to this command.
        if self.stale.replace(false) {
            self.discard_queued();
        }

        // The first byte is the report id; the board uses unnumbered reports,
        // so a leading zero is what it wants.
        let mut buf = [0u8; REPORT_SIZE + 1];
        buf[1..].copy_from_slice(out);
        future::block_on(self.dev.borrow_mut().write_output_report(&buf))?;

        let mut resp: Report = [0; REPORT_SIZE];
        let mut dev = self.dev.borrow_mut();
        // The library has no timed read, so the read races a timer.
        let read = future::block_on(future::or(
            async { Some(dev.read_input_report(&mut resp).await) },
            async {
                async_io::Timer::after(READ_TIMEOUT).await;
                None
            },
        ));
        drop(dev);

        match read {
            None => {
                self.stale.set(true);
                Err(Error::NoResponse)
            }
            Some(Err(e)) => Err(e.into()),
            Some(Ok(read)) if read != REPORT_SIZE => Err(Error::Device(format!(
                "Truncated answer from the board: {read} of {REPORT_SIZE} bytes"
            ))),
            Some(Ok(_)) => Ok(resp),
        }
    }
}

/// Serial numbers of all attached YKUR boards, in enumeration order.
pub fn list() -> Result<Vec<String>> {
    future::block_on(async {
        Ok(attached_boards()
            .await?
            .into_iter()
            .map(|(serial, _)| serial)
            .collect())
    })
}

/// What to tell a user whose operating system refused to open the board.
#[cfg(target_os = "linux")]
const PERMISSION_DENIED: &str = "Permission denied on the hidraw device of the YKUR. \
Allow access with a udev rule, /etc/udev/rules.d/60-ykur.rules as shown in the \
README, make this user a member of the group the rule names, and replug the board";
#[cfg(not(target_os = "linux"))]
const PERMISSION_DENIED: &str = "Permission denied on the YKUR";

/// Turns a refused open into an error that says what to do about it. On
/// Linux the board is `/dev/hidrawN`, owned by root until a udev rule says
/// otherwise, which the library reports as a plain `EACCES`.
fn open_failed(error: async_hid::HidError) -> Error {
    let denied = match &error {
        async_hid::HidError::Other(inner) => inner
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied),
        _ => false,
    };
    if denied {
        Error::Hid(PERMISSION_DENIED.to_owned())
    } else {
        error.into()
    }
}

async fn choose(serial: Option<&str>) -> Result<Device> {
    let attached = attached_boards().await?;
    let chosen = match serial {
        Some(wanted) => attached.into_iter().find(|(known, _)| known == wanted),
        None => attached.into_iter().next(),
    };
    chosen
        .map(|(_, device)| device)
        .ok_or_else(|| Error::NotFound {
            serial: serial.map(str::to_owned),
        })
}

async fn attached_boards() -> Result<Vec<(String, Device)>> {
    static BACKEND: OnceLock<HidBackend> = OnceLock::new();
    let backend = BACKEND.get_or_init(HidBackend::default);

    let mut found: Vec<(String, Device)> = Vec::new();
    let mut devices = backend.enumerate().await?;
    while let Some(dev) = devices.next().await {
        if dev.vendor_id != VENDOR_ID || dev.product_id != PRODUCT_ID {
            continue;
        }
        let serial = sanitize(dev.serial_number.as_deref().unwrap_or("<unknown>"));
        // A board with several HID interfaces would show up more than once.
        if !found.iter().any(|(known, _)| *known == serial) {
            found.push((serial, dev));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_the_payload_padded_with_zeros() {
        let r = report(&[0x01, 0x11]);
        assert_eq!(r.len(), REPORT_SIZE);
        assert_eq!(&r[..2], &[0x01, 0x11]);
        assert!(r[2..].iter().all(|&b| b == 0));
    }

    fn os_error(kind: std::io::ErrorKind) -> async_hid::HidError {
        async_hid::HidError::Other(Box::new(std::io::Error::from(kind)))
    }

    #[test]
    fn a_refused_open_says_what_to_do_about_it() {
        let error = open_failed(os_error(std::io::ErrorKind::PermissionDenied));
        assert!(matches!(&error, Error::Hid(msg) if msg == PERMISSION_DENIED));
        #[cfg(target_os = "linux")]
        assert!(error
            .to_string()
            .contains("/etc/udev/rules.d/60-ykur.rules"));
    }

    #[test]
    fn any_other_open_failure_keeps_its_own_message() {
        let error = open_failed(os_error(std::io::ErrorKind::NotFound));
        assert!(matches!(&error, Error::Hid(msg) if msg != PERMISSION_DENIED));
    }

    /// Needs a YKUR attached. Run with `cargo test -- --ignored --test-threads=1`.
    mod hardware {
        use super::*;

        #[test]
        #[ignore = "needs a YKUR attached"]
        fn enumeration_lists_the_attached_board() {
            assert!(!list().expect("enumeration should work").is_empty());
        }

        #[test]
        #[ignore = "needs a YKUR attached"]
        fn an_unknown_serial_number_is_reported_as_not_found() {
            assert!(matches!(
                Board::open(Some("no-such-board")),
                Err(Error::NotFound { serial: Some(_) })
            ));
        }
    }
}
