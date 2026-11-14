// SPDX-License-Identifier: Apache-2.0
//! Opening a YKUR on Linux: the turn first, then the handle, so this
//! process never has the board open while another one is in the middle of an
//! exchange.

use std::time::Duration;

use async_hid::{Device, DeviceReaderWriter};
use futures_lite::future;

use super::choose;
use super::nodes::hidraw_node;
use crate::error::{Error, Result};
use crate::lock::NodeLock;

/// A lock on the hidraw node of the board, see [`crate::lock`].
pub(super) type Turn = NodeLock;

/// What to tell a user whose operating system refused to open the board.
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the hidraw device of the YKUR. \
Allow access with a udev rule, /etc/udev/rules.d/60-ykur.rules as shown in the \
README, make this user a member of the group the rule names, and replug the board";

pub(super) fn open(serial: Option<&str>, wait: Duration) -> Result<(DeviceReaderWriter, Turn)> {
    let (_, device, turn) = crate::lock::take_stable(
        wait,
        || future::block_on(choose(serial)),
        hidraw_node_of,
        |node, found, left| NodeLock::take(node, found, left),
    )?;
    let dev = future::block_on(device.open())?;
    Ok((dev, turn))
}

/// The hidraw node of a board found by the transport.
///
/// On Linux the transport identifies a device by its directory in sysfs,
/// `.../hidraw/hidrawN`, and opens the node named in the `uevent` file there.
/// The lock goes on the same node.
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
