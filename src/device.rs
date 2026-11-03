// SPDX-License-Identifier: Apache-2.0
//! USB HID transport for the YKUR board.
//!
//! Talks to the board through async-hid. That library is asynchronous because
//! that is how macOS delivers HID reports; this program has no reason to be, so
//! the awaits are resolved here and everything above [`Transport`] stays
//! synchronous.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

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

/// How long a write or a read may take before the board counts as not
/// answering.
const TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait, after what looks like the answer to the version query,
/// for another report to follow. Long enough to catch one already on its
/// way, short enough not to be felt.
const SETTLE_WAIT: Duration = Duration::from_millis(50);

/// Upper bound on the reports dropped while getting back in step, so a board
/// that streams cannot hold the program there.
const DISCARD_LIMIT: usize = 16;

/// The firmware version query (datasheet 4.2.1). It changes nothing on the
/// board, and no other command is answered with these two bytes, so its
/// answer marks where the answers to earlier commands end.
pub(crate) const GET_VERSION: [u8; 2] = [0x20, 0x01];

pub type Report = [u8; REPORT_SIZE];

/// Report exchange with a board. The protocol layer is written against this
/// trait so it can be driven by a test double instead of real hardware.
pub trait Transport {
    /// Sends a report and returns the report the board answers with.
    fn transfer(&self, out: &Report) -> Result<Report>;

    /// Called when an answer did not fit its request: it may have been meant
    /// for an earlier one, so the next transfer must not trust the queue.
    fn desync(&self) {}
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

    /// Whether the next report to arrive is the answer to the next command.
    /// Not so on a fresh handle, which may find an answer an earlier process
    /// left behind, nor after a failed or unfitting exchange.
    in_step: Cell<bool>,

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
        // FreeBSD: hidraw takes one open at a time, so the handle is the turn
        // as on macOS. Listing opens the node too, so a held board is missing
        // from the list rather than refused.
        #[cfg(target_os = "freebsd")]
        let dev = {
            use crate::lock::{Attempt, HELD_OPEN};

            crate::lock::wait_for(serial.unwrap_or_default(), wait, || match future::block_on(
                choose(serial),
            ) {
                Ok((_, device)) => match future::block_on(device.open()) {
                    Ok(dev) => Attempt::Got(dev),
                    Err(error) if refused_as_busy(&error) => Attempt::Busy(HELD_OPEN.to_owned()),
                    Err(error) => Attempt::Failed(open_failed(error)),
                },
                Err(error @ Error::NotFound { .. }) => {
                    let Some((_, node)) = held_board(serial) else {
                        return Attempt::Failed(error);
                    };
                    // Listing skips a node it can not open, held or forbidden alike.
                    match std::fs::File::open(node) {
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                            Attempt::Failed(Error::PermissionDenied)
                        }
                        _ => Attempt::Busy(HELD_OPEN.to_owned()),
                    }
                }
                Err(error) => Attempt::Failed(error),
            })
            .map_err(|error| match error {
                Error::Busy { waited, cause, .. } => Error::Busy {
                    serial: held_board(serial)
                        .map(|(found, _)| found)
                        .unwrap_or_default(),
                    waited,
                    cause,
                },
                error => error,
            })?
        };

        Ok(Board {
            dev: RefCell::new(dev),
            in_step: Cell::new(false),
            #[cfg(target_os = "linux")]
            _turn: turn,
        })
    }
}

impl Transport for Board {
    fn transfer(&self, out: &Report) -> Result<Report> {
        exchange(&mut *self.dev.borrow_mut(), &self.in_step, out)
    }

    fn desync(&self) {
        self.in_step.set(false);
    }
}

/// One report in each direction, as the transport moves them; `None` when
/// the time ran out. The exchange below is written against this so that it
/// can be tested without a board.
trait Link {
    fn write(&mut self, out: &Report, timeout: Duration) -> Option<Result<()>>;
    fn read(&mut self, into: &mut Report, timeout: Duration) -> Option<Result<usize>>;
}

impl Link for DeviceReaderWriter {
    fn write(&mut self, out: &Report, timeout: Duration) -> Option<Result<()>> {
        // The first byte is the report id; the board uses unnumbered
        // reports, so a leading zero is what it wants.
        let mut buf = [0u8; REPORT_SIZE + 1];
        buf[1..].copy_from_slice(out);
        // The library has no timeouts, so each call races a timer.
        future::block_on(future::or(
            async { Some(self.write_output_report(&buf).await.map_err(Error::from)) },
            async {
                async_io::Timer::after(timeout).await;
                None
            },
        ))
    }

    fn read(&mut self, into: &mut Report, timeout: Duration) -> Option<Result<usize>> {
        future::block_on(future::or(
            async { Some(self.read_input_report(into).await.map_err(Error::from)) },
            async {
                async_io::Timer::after(timeout).await;
                None
            },
        ))
    }
}

/// Sends `out` and returns the answer, after getting back in step first when
/// `in_step` says the queue may hold answers to earlier commands. `in_step`
/// is set again only by a complete answer.
fn exchange(link: &mut impl Link, in_step: &Cell<bool>, out: &Report) -> Result<Report> {
    if !in_step.replace(false) {
        resync(link)?;
    }
    link.write(out, TIMEOUT).ok_or(Error::NoResponse)??;

    let mut answer: Report = [0; REPORT_SIZE];
    let read = link.read(&mut answer, TIMEOUT).ok_or(Error::NoResponse)??;
    full_report(read)?;
    in_step.set(true);
    Ok(answer)
}

/// Asks for the firmware version and drops every report up to its answer.
/// The board answers in order, so whatever was queued before - a late
/// answer, or one an earlier process left behind - comes first.
///
/// A version answer may itself be one left behind, by a process killed
/// during this very query. Taken too early, the real answer would wait in
/// the queue for the next command. So the queue has to stay quiet for a
/// moment after it; another report means the wait goes on.
fn resync(link: &mut impl Link) -> Result<()> {
    link.write(&report(&GET_VERSION), TIMEOUT)
        .ok_or(Error::NoResponse)??;

    let deadline = Instant::now() + TIMEOUT;
    let mut answer: Report = [0; REPORT_SIZE];
    let mut settling = false;
    // The reports dropped, the answer, and the quiet after it.
    for _ in 0..DISCARD_LIMIT + 2 {
        let wait = if settling {
            SETTLE_WAIT
        } else {
            deadline.saturating_duration_since(Instant::now())
        };
        let read = match link.read(&mut answer, wait) {
            None if settling => return Ok(()),
            None => return Err(Error::NoResponse),
            Some(read) => read?,
        };
        settling = read == REPORT_SIZE && answer[..2] == GET_VERSION;
    }
    Err(Error::Device(format!(
        "The board sent more than {DISCARD_LIMIT} reports before answering the version query"
    )))
}

/// Serial numbers of all attached YKUR boards, in enumeration order.
pub fn list() -> Result<Vec<String>> {
    #[allow(unused_mut)]
    let mut serials: Vec<String> = future::block_on(attached_boards())?
        .into_iter()
        .map(|(serial, _)| serial)
        .collect();
    // A board another program has open is missing from the device list.
    #[cfg(target_os = "freebsd")]
    for (held, _) in hidraw_boards() {
        if !serials.contains(&held) {
            serials.push(held);
        }
    }
    Ok(serials)
}

/// What to tell a user whose operating system refused to open the board.
#[cfg(target_os = "linux")]
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the hidraw device of the YKUR. \
Allow access with a udev rule, /etc/udev/rules.d/60-ykur.rules as shown in the \
README, make this user a member of the group the rule names, and replug the board";
#[cfg(target_os = "freebsd")]
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the hidraw device of the YKUR. \
Allow access with a devfs.rules entry as shown in the README and make this user a member of \
the group it names";
#[cfg(target_os = "macos")]
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
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn refused_as_busy(error: &async_hid::HidError) -> bool {
    matches!(error, async_hid::HidError::Busy)
}

/// The serial number of a YKUR in the `%pnpinfo` of a FreeBSD USB device, or
/// `None` for any other device.
#[cfg_attr(not(target_os = "freebsd"), allow(dead_code))]
fn ykur_serial(pnpinfo: &str) -> Option<String> {
    let field = |key: &str| {
        pnpinfo
            .split_whitespace()
            .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
    };
    let hex = |key: &str| u16::from_str_radix(field(key)?.strip_prefix("0x")?, 16).ok();

    (hex("vendor")? == VENDOR_ID && hex("product")? == PRODUCT_ID).then(|| {
        match field("sernum").unwrap_or("").trim_matches('"') {
            // Named as the device list names a board without one.
            "" => "<unknown>".to_owned(),
            sernum => sanitize(sernum),
        }
    })
}

/// The serial numbers and nodes of all YKUR boards with a hidraw node, read
/// through sysctl: listing opens the node, and FreeBSD lets one handle open
/// it at a time, so a board another program has open is missing from the
/// list.
#[cfg(target_os = "freebsd")]
fn hidraw_boards() -> Vec<(String, std::path::PathBuf)> {
    let sysctl = |name: String| {
        let out = std::process::Command::new("/sbin/sysctl")
            .args(["-n", &name])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    // "hidbus3" names the sysctl node "dev.hidbus.3".
    let node = |dev: &str| {
        let unit = dev.trim_start_matches(|c: char| !c.is_ascii_digit());
        format!("dev.{}.{unit}", &dev[..dev.len() - unit.len()])
    };

    let Ok(dev) = std::fs::read_dir("/dev") else {
        return Vec::new();
    };
    dev.filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("hidraw"))
        .filter_map(|hidraw| {
            // hidraw sits on hidbus, which sits on the USB driver.
            let hidbus = sysctl(format!("{}.%parent", node(&hidraw)))?;
            let usb = sysctl(format!("{}.%parent", node(&hidbus)))?;
            let serial = ykur_serial(&sysctl(format!("{}.%pnpinfo", node(&usb)))?)?;
            Some((serial, std::path::Path::new("/dev").join(hidraw)))
        })
        .collect()
}

/// The board with `serial`, or any board, among those with a hidraw node.
#[cfg(target_os = "freebsd")]
fn held_board(serial: Option<&str>) -> Option<(String, std::path::PathBuf)> {
    hidraw_boards()
        .into_iter()
        .find(|(found, _)| serial.is_none_or(|wanted| found == wanted))
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
    fn a_board_is_recognised_in_the_pnp_info_of_freebsd() {
        let ykur = r#"vendor=0x04d8 product=0xf1cb devclass=0x00 sernum="YK22170" release=0x0100"#;
        assert_eq!(ykur_serial(ykur), Some("YK22170".to_owned()));
        let mouse = r#"vendor=0x0e0f product=0x0003 sernum="" release=0x0102"#;
        assert_eq!(ykur_serial(mouse), None);
        let unnamed = r#"vendor=0x04d8 product=0xf1cb sernum="""#;
        assert_eq!(ykur_serial(unnamed), Some("<unknown>".to_owned()));
        assert_eq!(ykur_serial("pci0:0:0:0"), None);
    }

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

    /// What a [`FakeLink`] read returns next.
    enum Incoming {
        Answer(Report),
        Short(usize),
        Fails,
        Silent,
    }

    /// A link that records what is written and reads from a prepared queue;
    /// an empty queue reads as silence.
    #[derive(Default)]
    struct FakeLink {
        written: Vec<Report>,
        incoming: std::collections::VecDeque<Incoming>,
        write_hangs: bool,
    }

    impl FakeLink {
        fn answers(payloads: &[&[u8]]) -> Self {
            let mut link = FakeLink::default();
            for payload in payloads {
                link.incoming.push_back(Incoming::Answer(report(payload)));
            }
            link
        }

        fn then(mut self, incoming: Incoming) -> Self {
            self.incoming.push_back(incoming);
            self
        }

        fn written(&self) -> Vec<Vec<u8>> {
            self.written.iter().map(|r| r[..2].to_vec()).collect()
        }
    }

    impl Link for FakeLink {
        fn write(&mut self, out: &Report, _: Duration) -> Option<Result<()>> {
            if self.write_hangs {
                return None;
            }
            self.written.push(*out);
            Some(Ok(()))
        }

        fn read(&mut self, into: &mut Report, _: Duration) -> Option<Result<usize>> {
            match self.incoming.pop_front().unwrap_or(Incoming::Silent) {
                Incoming::Answer(r) => {
                    *into = r;
                    Some(Ok(REPORT_SIZE))
                }
                Incoming::Short(n) => Some(Ok(n)),
                Incoming::Fails => Some(Err(Error::Hid("read failed".into()))),
                Incoming::Silent => None,
            }
        }
    }

    const STATE_1: [u8; 2] = [0x03, 0x01];
    const VERSION: &[u8] = &[0x20, 0x01, 1, 2, 1];

    #[test]
    fn a_fresh_handle_drops_an_answer_left_behind_by_an_earlier_process() {
        // A process ended between its write and its read; its answer comes
        // before anything this one asks.
        let mut link = FakeLink::answers(&[&[0x03, 0x01, 0x01], VERSION]);
        link.incoming.push_back(Incoming::Silent);
        link.incoming
            .push_back(Incoming::Answer(report(&[0x03, 0x01, 0x00])));
        let in_step = Cell::new(false);

        let answer = exchange(&mut link, &in_step, &report(&STATE_1)).unwrap();

        assert_eq!(answer[..3], [0x03, 0x01, 0x00]);
        assert_eq!(link.written(), [GET_VERSION.to_vec(), STATE_1.to_vec()]);
        assert!(in_step.get());
    }

    #[test]
    fn a_version_answer_left_behind_does_not_end_the_resync_early() {
        // An earlier process was killed during its own version query; taken
        // for the answer, the real one would be read as the answer to the
        // command that follows.
        let mut link = FakeLink::answers(&[VERSION, VERSION]);
        link.incoming.push_back(Incoming::Silent);
        link.incoming
            .push_back(Incoming::Answer(report(&[0x03, 0x01, 0x00])));
        let in_step = Cell::new(false);

        let answer = exchange(&mut link, &in_step, &report(&STATE_1)).unwrap();

        assert_eq!(answer[..3], [0x03, 0x01, 0x00]);
        assert!(link.incoming.is_empty());
    }

    #[test]
    fn a_handle_in_step_sends_the_command_alone() {
        let mut link = FakeLink::answers(&[&[0x03, 0x01, 0x01]]);
        let in_step = Cell::new(true);

        exchange(&mut link, &in_step, &report(&STATE_1)).unwrap();

        assert_eq!(link.written(), [STATE_1.to_vec()]);
        assert!(in_step.get());
    }

    #[test]
    fn every_failed_exchange_leaves_the_handle_out_of_step() {
        for (what, incoming) in [
            ("timeout", Incoming::Silent),
            ("short report", Incoming::Short(10)),
            ("read error", Incoming::Fails),
        ] {
            let mut link = FakeLink::default().then(incoming);
            let in_step = Cell::new(true);

            assert!(
                exchange(&mut link, &in_step, &report(&STATE_1)).is_err(),
                "{what}"
            );
            assert!(!in_step.get(), "{what}");
        }
    }

    #[test]
    fn a_late_answer_is_dropped_by_the_next_exchange() {
        let mut link = FakeLink::default()
            .then(Incoming::Silent)
            .then(Incoming::Answer(report(&[0x03, 0x01, 0x01])))
            .then(Incoming::Answer(report(VERSION)))
            .then(Incoming::Silent)
            .then(Incoming::Answer(report(&[0x03, 0x02, 0x00])));
        let in_step = Cell::new(true);

        assert!(matches!(
            exchange(&mut link, &in_step, &report(&STATE_1)),
            Err(Error::NoResponse)
        ));
        let answer = exchange(&mut link, &in_step, &report(&[0x03, 0x02])).unwrap();

        assert_eq!(answer[..3], [0x03, 0x02, 0x00]);
    }

    #[test]
    fn a_short_report_while_getting_in_step_is_dropped() {
        let mut link = FakeLink::default()
            .then(Incoming::Short(2))
            .then(Incoming::Answer(report(VERSION)))
            .then(Incoming::Silent)
            .then(Incoming::Answer(report(&[0x03, 0x01, 0x01])));
        let in_step = Cell::new(false);

        let answer = exchange(&mut link, &in_step, &report(&STATE_1)).unwrap();

        assert_eq!(answer[..3], [0x03, 0x01, 0x01]);
    }

    #[test]
    fn a_read_error_while_getting_in_step_is_reported() {
        let mut link = FakeLink::default().then(Incoming::Fails);
        let in_step = Cell::new(false);

        assert!(matches!(
            exchange(&mut link, &in_step, &report(&STATE_1)),
            Err(Error::Hid(_))
        ));
        assert_eq!(link.written(), [GET_VERSION.to_vec()]);
        assert!(!in_step.get());
    }

    #[test]
    fn a_write_that_does_not_finish_in_time_is_no_response() {
        let mut link = FakeLink {
            write_hangs: true,
            ..FakeLink::default()
        };
        let in_step = Cell::new(true);

        assert!(matches!(
            exchange(&mut link, &in_step, &report(&STATE_1)),
            Err(Error::NoResponse)
        ));
        assert!(!in_step.get());
    }

    #[test]
    fn a_board_that_does_not_answer_the_version_query_is_no_response() {
        let mut link = FakeLink::default();
        let in_step = Cell::new(false);

        assert!(matches!(
            exchange(&mut link, &in_step, &report(&STATE_1)),
            Err(Error::NoResponse)
        ));
        assert_eq!(link.written(), [GET_VERSION.to_vec()]);
        assert!(!in_step.get());
    }

    #[test]
    fn as_many_reports_as_the_limit_allows_are_dropped() {
        let mut link = FakeLink::default();
        for _ in 0..DISCARD_LIMIT {
            link.incoming
                .push_back(Incoming::Answer(report(&[0x03, 0x01, 0x01])));
        }
        link.incoming.push_back(Incoming::Answer(report(VERSION)));
        link.incoming.push_back(Incoming::Silent);
        link.incoming
            .push_back(Incoming::Answer(report(&[0x03, 0x01, 0x00])));
        let in_step = Cell::new(false);

        let answer = exchange(&mut link, &in_step, &report(&STATE_1)).unwrap();

        assert_eq!(answer[..3], [0x03, 0x01, 0x00]);
    }

    #[test]
    fn a_board_that_never_gets_to_the_version_answer_is_given_up_on() {
        let mut link = FakeLink::default();
        for _ in 0..=DISCARD_LIMIT {
            link.incoming
                .push_back(Incoming::Answer(report(&[0x03, 0x01, 0x01])));
        }
        link.incoming.push_back(Incoming::Answer(report(VERSION)));
        let in_step = Cell::new(false);

        assert!(matches!(
            exchange(&mut link, &in_step, &report(&STATE_1)),
            Err(Error::Device(_))
        ));
        assert!(!in_step.get());
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
