// SPDX-License-Identifier: Apache-2.0
//! Opening a YKUR on macOS: the handle itself is the turn. The board is
//! opened exclusively (seized), so every other open fails with Busy until
//! this handle is dropped, and such an open is repeated.

use std::time::Duration;

use async_hid::{Access, DeviceReaderWriter};
use futures_lite::future;

use super::choose;
use super::waiting::Seize;
use crate::error::Result;

/// Nothing beyond the handle, which holds the board exclusively.
pub(super) type Turn = ();

/// What to tell a user whose operating system refused to open the board.
pub(crate) const PERMISSION_DENIED: &str = "Permission denied on the YKUR";

pub(super) fn open(serial: Option<&str>, wait: Duration) -> Result<(DeviceReaderWriter, Turn)> {
    let (found, device) = future::block_on(choose(serial))?;
    let mut seize = Seize::new(device, crate::lock::HELD_EXCLUSIVELY);
    let dev = crate::lock::wait_for(&found, wait, || {
        seize.attempt(
            || future::block_on(choose(Some(&found))).map(|(_, again)| again),
            |device| future::block_on(device.open_options().access(Access::Exclusive).open()),
        )
    })?;
    Ok((dev, ()))
}
