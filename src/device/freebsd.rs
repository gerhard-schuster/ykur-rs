// SPDX-License-Identifier: Apache-2.0
//! Opening a YKUR on FreeBSD: hidraw takes one open at a time, so the
//! handle is the turn as on macOS. Listing opens the node too, so a held
//! board is missing from the list rather than refused.

use std::time::Duration;

use async_hid::{Device, DeviceReaderWriter};
use futures_lite::future;

use super::nodes::{is_hidraw_name, ykur_serial};
use super::waiting::NodeWait;
use super::{backend, choose, PRODUCT_ID, VENDOR_ID};
use crate::error::{Error, Result};
use crate::lock::HELD_OPEN;
use crate::sanitize::sanitize;

/// Nothing beyond the handle, which no other process can open meanwhile.
pub(super) type Turn = ();

/// What to tell a user whose operating system refused to open the board.
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the hidraw device of the YKUR. \
Allow access with a devfs.rules entry as shown in the README and make this user a member of \
the group it names";

pub(super) fn open(serial: Option<&str>, wait: Duration) -> Result<(DeviceReaderWriter, Turn)> {
    // Without a serial number, the first board by node, held or not.
    let first = serial
        .is_none()
        .then(|| held_board(None))
        .flatten()
        .map(|(found, _)| found);
    let serial = serial.or(first.as_deref());

    let mut node_wait = NodeWait::default();
    let dev = crate::lock::wait_for(serial.unwrap_or_default(), wait, || {
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
    })?;
    Ok((dev, ()))
}

/// The serial numbers and nodes of all YKUR boards with a hidraw node, read
/// through sysctl: listing opens the node, and FreeBSD lets one handle open
/// it at a time, so a board another program has open is missing from the
/// list.
pub(super) fn hidraw_boards() -> Vec<(String, std::path::PathBuf)> {
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
fn held_board(serial: Option<&str>) -> Option<(String, std::path::PathBuf)> {
    hidraw_boards()
        .into_iter()
        .find(|(found, _)| serial.is_none_or(|wanted| found == wanted))
}
