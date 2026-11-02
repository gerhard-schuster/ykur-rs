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

#[cfg(target_os = "macos")]
use async_hid::Access;
use async_hid::{AsyncHidRead, AsyncHidWrite, Device, DeviceReaderWriter, HidBackend};
use futures_lite::{future, StreamExt};

use crate::error::{Error, Result};
pub use crate::lock::DEFAULT_WAIT;
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
    /// shared reference; a board is driven from one thread at a time. On
    /// macOS the handle holds the board exclusively.
    dev: RefCell<DeviceReaderWriter>,

    /// Set when a read timed out. The answer may still arrive afterwards and
    /// would then be queued, so the next exchange empties the queue first.
    stale: Cell<bool>,

    /// This process's turn at the board, see [`crate::lock`]. Declared last
    /// so that it is dropped last: the next invocation must not get the turn
    /// while this one still has the device open.
    #[cfg(target_os = "linux")]
    _turn: crate::lock::NodeLock,
}

impl Board {
    /// Opens the board with the given serial number, or the first one found.
    ///
    /// A board another invocation is using is waited for, up to
    /// [`DEFAULT_WAIT`]; see [`crate::lock`].
    pub fn open(serial: Option<&str>) -> Result<Self> {
        Board::open_within(serial, DEFAULT_WAIT)
    }

    /// Like [`Board::open`], waiting at most `wait` for a board another
    /// invocation is using. With `wait` zero, a board in use fails at once
    /// with [`Error::Busy`].
    pub fn open_within(serial: Option<&str>, wait: Duration) -> Result<Self> {
        // Linux: the turn first, then the handle, so this process never has
        // the board open while another one is in the middle of an exchange.
        #[cfg(target_os = "linux")]
        let (turn, device) = {
            let (_, device, turn) = crate::lock::take_stable(
                wait,
                || future::block_on(choose(serial)),
                hidraw_node_of,
                |node, found, left| crate::lock::NodeLock::take(node, found, left),
            )?;
            (turn, device)
        };
        #[cfg(target_os = "linux")]
        let dev = future::block_on(device.open()).map_err(open_failed)?;

        // macOS: the handle itself is the turn. The board is opened
        // exclusively (seized), so every other open fails with Busy until
        // this handle is dropped, and such an open is repeated.
        #[cfg(target_os = "macos")]
        let dev = {
            use crate::lock::Attempt;

            let (found, device) = future::block_on(choose(serial))?;
            crate::lock::wait_for(&found, wait, || {
                match future::block_on(device.open_options().access(Access::Exclusive).open()) {
                    Ok(dev) => Attempt::Got(dev),
                    Err(error) if refused_as_busy(&error) => {
                        Attempt::Busy(crate::lock::HELD_EXCLUSIVELY.to_owned())
                    }
                    Err(error) => Attempt::Failed(open_failed(error)),
                }
            })?
        };
        Ok(Board {
            dev: RefCell::new(dev),
            stale: Cell::new(false),
            #[cfg(target_os = "linux")]
            _turn: turn,
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
            Some(Ok(read)) => full_report(read).map(|()| resp),
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
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the hidraw device of the YKUR. \
Allow access with a udev rule, /etc/udev/rules.d/60-ykur.rules as shown in the \
README, make this user a member of the group the rule names, and replug the board";
#[cfg(not(target_os = "linux"))]
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the YKUR";

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
        Error::PermissionDenied
    } else {
        error.into()
    }
}

/// Whether an open failed because another handle holds the board
/// exclusively, which waiting may cure. The transport reports exactly that
/// as `Busy` (`kIOReturnExclusiveAccess` on macOS); every other failure is
/// reported at once.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn refused_as_busy(error: &async_hid::HidError) -> bool {
    matches!(error, async_hid::HidError::Busy)
}

/// The board with the given serial number, or the first one found, with its
/// sanitised serial number.
async fn choose(serial: Option<&str>) -> Result<(String, Device)> {
    let attached = attached_boards().await?;
    let chosen = match serial {
        Some(wanted) => attached.into_iter().find(|(known, _)| known == wanted),
        None => attached.into_iter().next(),
    };
    chosen.ok_or_else(|| Error::NotFound {
        serial: serial.map(str::to_owned),
    })
}

/// The hidraw node of a board found by the transport.
///
/// On Linux the transport identifies a device by its directory in sysfs,
/// `.../hidraw/hidrawN`, and opens the node named in the `uevent` file there.
/// The lock goes on the same node.
#[cfg(target_os = "linux")]
fn hidraw_node_of(device: &Device) -> Result<std::path::PathBuf> {
    let async_hid::DeviceId::DevPath(sysfs) = &device.id else {
        return Err(Error::Unavailable("The YKUR has no hidraw device".into()));
    };

    hidraw_node(sysfs).ok_or_else(|| {
        Error::Unavailable(format!(
            "No hidraw device node found for the YKUR at {}",
            sysfs.display()
        ))
    })
}

/// Reads `DEVNAME` from the `uevent` file in a hidraw directory of sysfs and
/// turns it into the path under `/dev`. Only a plain `hidrawN` is taken, so a
/// file that says something else cannot point the lock anywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hidraw_node(sysfs: &std::path::Path) -> Option<std::path::PathBuf> {
    let uevent = std::fs::read_to_string(sysfs.join("uevent")).ok()?;
    let name = uevent
        .lines()
        .find_map(|line| line.strip_prefix("DEVNAME="))?
        .trim();
    let number = name.strip_prefix("hidraw")?;

    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        .then(|| std::path::Path::new("/dev").join(name))
}

async fn attached_boards() -> Result<Vec<(String, Device)>> {
    static BACKEND: OnceLock<HidBackend> = OnceLock::new();
    let backend = BACKEND.get_or_init(HidBackend::default);

    let mut found: Vec<(String, Device)> = Vec::new();
    // A HID stack that cannot even list its devices is an unavailable
    // service, not an I/O error that a retry would cure.
    let mut devices = backend
        .enumerate()
        .await
        .map_err(|e| Error::Unavailable(format!("Cannot list the HID devices: {e}")))?;
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

/// A report shorter than 64 bytes is a failed transfer: the read came back
/// short, and reading the zero padding of the buffer as answer bytes would
/// turn it into a wrong answer. It is an I/O error, not an answer that does
/// not fit the request.
fn full_report(read: usize) -> Result<()> {
    if read == REPORT_SIZE {
        Ok(())
    } else {
        Err(Error::Hid(format!(
            "Truncated answer from the board: {read} of {REPORT_SIZE} bytes"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_truncated_report_is_an_io_error_not_an_unfitting_answer() {
        assert!(full_report(REPORT_SIZE).is_ok());
        for read in [0, 1, REPORT_SIZE - 1] {
            let error = full_report(read).unwrap_err();
            assert!(matches!(error, Error::Hid(_)), "{read}: {error:?}");
            assert_eq!(error.exit_code(), 74);
        }
    }

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
        assert!(matches!(&error, Error::PermissionDenied));
        assert_eq!(error.to_string(), PERMISSION_DENIED);
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

    /// A directory in the temporary directory standing in for sysfs,
    /// removed again when dropped.
    struct FakeSysfs(std::path::PathBuf);

    impl FakeSysfs {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ykur-sysfs-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            FakeSysfs(dir)
        }

        fn file(&self, path: &str, content: &str) -> &Self {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
            self
        }
    }

    impl Drop for FakeSysfs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_busy_from_the_transport_counts_as_a_board_in_use() {
        use async_hid::HidError;

        assert!(refused_as_busy(&HidError::Busy));
        assert!(!refused_as_busy(&HidError::message(
            "Failed to open device"
        )));
        assert!(!refused_as_busy(&HidError::NotConnected));
        assert!(!refused_as_busy(&HidError::Unsupported));
        assert!(!refused_as_busy(&os_error(
            std::io::ErrorKind::PermissionDenied
        )));
    }

    #[test]
    fn the_hidraw_node_is_read_from_sysfs() {
        let sysfs = FakeSysfs::new("node");
        sysfs.file(
            "hidraw/hidraw3/uevent",
            "MAJOR=241\nMINOR=3\nDEVNAME=hidraw3",
        );

        assert_eq!(
            hidraw_node(&sysfs.0.join("hidraw/hidraw3")),
            Some(std::path::PathBuf::from("/dev/hidraw3"))
        );
    }

    #[test]
    fn a_node_name_that_is_not_hidraw_is_refused() {
        let sysfs = FakeSysfs::new("badnode");
        sysfs
            .file("a/uevent", "DEVNAME=../sda")
            .file("b/uevent", "DEVNAME=hidraw")
            .file("c/uevent", "DEVNAME=hidraw1/../../sda")
            .file("d/uevent", "MAJOR=241");

        for dir in ["a", "b", "c", "d", "missing"] {
            assert_eq!(hidraw_node(&sysfs.0.join(dir)), None, "{dir}");
        }
    }

    /// Needs a YKUR attached. Run with `cargo test -- --ignored --test-threads=1`.
    mod hardware {
        use super::*;

        /// A second invocation waits for the first; with no wait it is
        /// refused as busy. On Linux that is the flock, on macOS the
        /// exclusive open (passes since 2026-11-02).
        /// Read-only: nothing is sent to the board.
        #[test]
        #[ignore = "needs a YKUR attached"]
        fn a_held_board_is_not_opened_a_second_time() {
            let first = Board::open(None).expect("the first board should open");
            let second = Board::open_within(None, Duration::ZERO);

            assert!(
                matches!(&second, Err(Error::Busy { .. })),
                "a second open should be refused as busy, got {:?}",
                second.as_ref().err()
            );
            drop(second);
            drop(first);
            Board::open_within(None, Duration::ZERO)
                .expect("the board should open again once the first handle is gone");
        }

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
