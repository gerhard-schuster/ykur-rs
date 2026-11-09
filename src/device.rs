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
use crate::lock::Attempt;
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
        let dev = future::block_on(device.open())?;

        // macOS: the handle itself is the turn. The board is opened
        // exclusively (seized), so every other open fails with Busy until
        // this handle is dropped, and such an open is repeated.
        #[cfg(target_os = "macos")]
        let dev = {
            let (found, device) = future::block_on(choose(serial))?;
            let mut seize = Seize::new(device, crate::lock::HELD_EXCLUSIVELY);
            crate::lock::wait_for(&found, wait, || {
                seize.attempt(
                    || future::block_on(choose(Some(&found))).map(|(_, again)| again),
                    |device| {
                        future::block_on(device.open_options().access(Access::Exclusive).open())
                    },
                )
            })?
        };
        // FreeBSD: hidraw takes one open at a time, so the handle is the turn
        // as on macOS. Listing opens the node too, so a held board is missing
        // from the list rather than refused.
        #[cfg(target_os = "freebsd")]
        let dev = {
            use crate::lock::HELD_OPEN;

            // Without a serial number, the first board by node, held or not.
            let first = serial
                .is_none()
                .then(|| held_board(None))
                .flatten()
                .map(|(found, _)| found);
            let serial = serial.or(first.as_deref());

            let mut node_wait = NodeWait::default();
            crate::lock::wait_for(serial.unwrap_or_default(), wait, || {
                node_wait.attempt(
                    || future::block_on(choose(serial)).map(|(_, device)| device),
                    || held_board(serial),
                    |node, found| future::block_on(node_device(node, found)),
                    |device: Device| future::block_on(device.open()),
                    HELD_OPEN,
                )
            })
            .map_err(|error| match error {
                Error::Busy { waited, cause, .. } => Error::Busy {
                    serial: held_board(serial)
                        .map(|(found, _)| found)
                        .or_else(|| serial.map(str::to_owned))
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
        timed(timeout, self.write_output_report(&buf))
    }

    fn read(&mut self, into: &mut Report, timeout: Duration) -> Option<Result<usize>> {
        timed(timeout, self.read_input_report(into))
    }
}

/// Races a transport call against `timeout`, since the library has none.
fn timed<T>(
    timeout: Duration,
    call: impl std::future::Future<Output = async_hid::HidResult<T>>,
) -> Option<Result<T>> {
    future::block_on(future::or(
        async { Some(call.await.map_err(Error::from)) },
        async {
            async_io::Timer::after(timeout).await;
            None
        },
    ))
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

/// Serial numbers of all attached YKUR boards, in enumeration order; on
/// FreeBSD sorted by the path of their device nodes.
pub fn list() -> Result<Vec<String>> {
    let attached = future::block_on(attached_boards())?
        .into_iter()
        .map(|(serial, _)| serial);
    // A board another program has open is missing from the device list.
    #[cfg(target_os = "freebsd")]
    let attached = hidraw_boards()
        .into_iter()
        .map(|(serial, _)| serial)
        .chain(attached);

    let mut serials: Vec<String> = Vec::new();
    for serial in attached {
        if !serials.contains(&serial) {
            serials.push(serial);
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

/// Waiting for a board opened exclusively, on macOS, kept between attempts.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Seize<D> {
    /// None while the board re-enumerates, e.g. reset by the holder.
    device: Option<D>,
    /// Once it was held elsewhere, the holder may have reset it.
    may_reset: bool,
    busy: &'static str,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl<D> Seize<D> {
    fn new(device: D, busy: &'static str) -> Self {
        Seize {
            device: Some(device),
            may_reset: false,
            busy,
        }
    }

    fn attempt<T>(
        &mut self,
        choose: impl FnOnce() -> Result<D>,
        open: impl FnOnce(&D) -> async_hid::HidResult<T>,
    ) -> Attempt<T> {
        let current = match self.device.take() {
            Some(current) => current,
            None => match choose() {
                Ok(again) => again,
                Err(error @ Error::NotFound { .. }) => return Attempt::Gone(error),
                Err(error) => return Attempt::Failed(error),
            },
        };
        match open(&current) {
            Ok(dev) => Attempt::Got(dev),
            Err(error) if refused_as_busy(&error) => {
                self.device = Some(current);
                self.may_reset = true;
                Attempt::Busy(self.busy.to_owned())
            }
            // IOKit names a vanished or returning board in several ways.
            Err(error) if self.may_reset || matches!(error, async_hid::HidError::NotConnected) => {
                Attempt::Gone(error.into())
            }
            Err(error) => Attempt::Failed(error.into()),
        }
    }
}

/// Waiting for a board on FreeBSD, where hidraw takes one open at a time,
/// kept between attempts.
#[derive(Default)]
#[cfg_attr(not(target_os = "freebsd"), allow(dead_code))]
struct NodeWait {
    /// Once the board is missing from the list, only its node is tried.
    held: Option<(String, std::path::PathBuf)>,
    /// Once it was gone, e.g. reset by the holder, it may be coming back.
    gone: bool,
}

#[cfg_attr(not(target_os = "freebsd"), allow(dead_code))]
impl NodeWait {
    fn attempt<D, T>(
        &mut self,
        choose: impl FnOnce() -> Result<D>,
        held_board: impl FnOnce() -> Option<(String, std::path::PathBuf)>,
        node_device: impl FnOnce(&std::path::Path, &str) -> async_hid::HidResult<D>,
        open: impl FnOnce(D) -> async_hid::HidResult<T>,
        busy: &str,
    ) -> Attempt<T> {
        let device = match self.held.take() {
            Some(board) => self.query(board, node_device),
            None => match choose() {
                Ok(device) => Ok(device),
                Err(error @ Error::NotFound { .. }) => match held_board() {
                    Some(board) => self.query(board, node_device),
                    None if self.gone => return Attempt::Gone(error),
                    None => return Attempt::Failed(error),
                },
                Err(error) => return Attempt::Failed(error),
            },
        };
        match device.and_then(open) {
            Ok(dev) => Attempt::Got(dev),
            Err(error) if refused_as_busy(&error) => Attempt::Busy(busy.to_owned()),
            Err(error @ async_hid::HidError::NotConnected) => {
                self.gone = true;
                self.held = None;
                Attempt::Gone(error.into())
            }
            Err(error) => Attempt::Failed(error.into()),
        }
    }

    fn query<D>(
        &mut self,
        (found, node): (String, std::path::PathBuf),
        node_device: impl FnOnce(&std::path::Path, &str) -> async_hid::HidResult<D>,
    ) -> async_hid::HidResult<D> {
        let device = node_device(&node, &found);
        self.held = Some((found, node));
        device
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
    let mut boards: Vec<_> = dev
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| is_hidraw_name(name))
        .filter_map(|hidraw| {
            // hidraw sits on hidbus, which sits on the USB driver.
            let hidbus = sysctl(format!("{}.%parent", node(&hidraw)))?;
            let usb = sysctl(format!("{}.%parent", node(&hidbus)))?;
            let serial = ykur_serial(&sysctl(format!("{}.%pnpinfo", node(&usb)))?)?;
            Some((serial, std::path::Path::new("/dev").join(hidraw)))
        })
        .collect();
    // In the node order of the device list.
    boards.sort_by(|a, b| a.1.cmp(&b.1));
    boards
}

/// The board `serial` on `node`, read without listing the other nodes.
#[cfg(target_os = "freebsd")]
async fn node_device(node: &std::path::Path, serial: &str) -> async_hid::HidResult<Device> {
    let id = async_hid::DeviceId::DevPath(node.to_path_buf());
    backend()
        .query_devices(&id)
        .await?
        .find(|dev| {
            dev.vendor_id == VENDOR_ID
                && dev.product_id == PRODUCT_ID
                && sanitize(dev.serial_number.as_deref().unwrap_or("<unknown>")) == serial
        })
        .ok_or(async_hid::HidError::NotConnected)
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

    is_hidraw_name(name).then(|| std::path::Path::new("/dev").join(name))
}

/// Whether `name` is `hidraw` followed by one or more ASCII digits.
fn is_hidraw_name(name: &str) -> bool {
    name.strip_prefix("hidraw")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// One backend for the process, made once instead of per call.
fn backend() -> &'static HidBackend {
    static BACKEND: OnceLock<HidBackend> = OnceLock::new();
    BACKEND.get_or_init(HidBackend::default)
}

async fn attached_boards() -> Result<Vec<(String, Device)>> {
    let mut found: Vec<(String, Device)> = Vec::new();
    // A HID stack that cannot even list its devices is an unavailable
    // service, not an I/O error that a retry would cure.
    let mut devices = backend()
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
        let error = Error::from(os_error(std::io::ErrorKind::PermissionDenied));
        assert!(matches!(&error, Error::PermissionDenied));
        assert_eq!(error.to_string(), PERMISSION_DENIED);
        #[cfg(target_os = "linux")]
        assert!(error
            .to_string()
            .contains("/etc/udev/rules.d/60-ykur.rules"));
    }

    #[test]
    fn any_other_open_failure_keeps_its_own_message() {
        let error = Error::from(os_error(std::io::ErrorKind::NotFound));
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

#[cfg(test)]
mod waiting_tests {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};

    use async_hid::HidError;

    use super::{NodeWait, Seize};
    use crate::error::Error;
    use crate::lock::Attempt;

    fn not_found() -> crate::error::Result<&'static str> {
        Err(Error::NotFound { serial: None })
    }

    fn unused<R>() -> R {
        panic!("not expected in this attempt")
    }

    fn outcome<T>(attempt: Attempt<T>) -> &'static str {
        match attempt {
            Attempt::Got(_) => "got",
            Attempt::Busy(_) => "busy",
            Attempt::Gone(_) => "gone",
            Attempt::Failed(_) => "failed",
        }
    }

    // -- macOS: the seized handle ------------------------------------------

    #[test]
    fn an_open_failure_before_the_board_was_busy_fails_at_once() {
        let mut seize = Seize::new("old", "held");

        let attempt = seize.attempt(unused, |_| Err::<(), _>(HidError::message("x")));

        assert_eq!(outcome(attempt), "failed");
    }

    #[test]
    fn a_board_reset_while_held_elsewhere_is_found_again() {
        let mut seize = Seize::new("old", "held");
        let opened = Cell::new(None);

        let busy = seize.attempt(unused, |_| Err::<(), _>(HidError::Busy));
        let vanished = seize.attempt(unused, |_| Err::<(), _>(HidError::message("gone")));
        let missing = seize.attempt(not_found, |_| unused());
        let back = seize.attempt(
            || Ok("new"),
            |device| {
                opened.set(Some(*device));
                Ok(())
            },
        );

        let seen = [busy, vanished, missing, back].map(outcome);
        assert_eq!(seen, ["busy", "gone", "gone", "got"]);
        assert_eq!(opened.get(), Some("new"));
    }

    #[test]
    fn a_board_gone_at_the_first_open_is_looked_for_again() {
        let mut seize = Seize::new("old", "held");

        let gone = seize.attempt(unused, |_| Err::<(), _>(HidError::NotConnected));
        let back = seize.attempt(|| Ok("new"), |_| Ok(()));

        assert_eq!([gone, back].map(outcome), ["gone", "got"]);
    }

    // -- FreeBSD: one open per node -----------------------------------------

    fn board() -> Option<(String, PathBuf)> {
        Some(("YK1".to_owned(), PathBuf::from("/dev/hidraw3")))
    }

    #[test]
    fn a_board_neither_listed_nor_in_sysctl_is_not_found_at_once() {
        let mut wait = NodeWait::default();

        let attempt = wait.attempt(
            not_found,
            || None,
            |_, _| unused(),
            |_| unused::<Result<(), _>>(),
            "held",
        );

        assert!(matches!(attempt, Attempt::Failed(Error::NotFound { .. })));
    }

    #[test]
    fn a_held_board_is_asked_for_by_its_node_only() {
        let mut wait = NodeWait::default();
        let asked = Cell::new(0);
        let node = |path: &Path, serial: &str| {
            assert_eq!((path, serial), (Path::new("/dev/hidraw3"), "YK1"));
            asked.set(asked.get() + 1);
            Err::<&str, _>(HidError::Busy)
        };

        let first = wait.attempt(
            not_found,
            board,
            node,
            |_| unused::<Result<(), _>>(),
            "held",
        );
        let second = wait.attempt(unused, unused, node, |_| unused::<Result<(), _>>(), "held");

        assert_eq!([first, second].map(outcome), ["busy", "busy"]);
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn a_board_that_left_its_node_is_waited_for_until_it_is_back() {
        let mut wait = NodeWait::default();

        let held = wait.attempt(
            not_found,
            board,
            |_, _| Err::<&str, _>(HidError::Busy),
            |_| unused::<Result<(), _>>(),
            "held",
        );
        let left = wait.attempt(
            unused,
            unused,
            |_, _| Err::<&str, _>(HidError::NotConnected),
            |_| unused::<Result<(), _>>(),
            "held",
        );
        let missing = wait.attempt(
            not_found,
            || None,
            |_, _| unused(),
            |_| unused::<Result<(), _>>(),
            "held",
        );
        let back = wait.attempt(|| Ok("new"), unused, |_, _| unused(), |_| Ok(()), "held");

        assert_eq!(
            [held, left, missing, back].map(outcome),
            ["busy", "gone", "gone", "got"]
        );
    }

    #[test]
    fn a_listed_board_that_is_gone_at_the_open_is_waited_for() {
        let mut wait = NodeWait::default();

        let attempt = wait.attempt(
            || Ok("old"),
            unused,
            |_, _| unused(),
            |_| Err::<(), _>(HidError::NotConnected),
            "held",
        );

        assert_eq!(outcome(attempt), "gone");
    }
}
