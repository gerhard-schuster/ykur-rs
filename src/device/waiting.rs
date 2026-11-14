// SPDX-License-Identifier: Apache-2.0
//! Waiting for a board another handle holds, on the systems where the
//! handle itself is the turn: macOS and FreeBSD. Built on every system, so
//! that its tests run everywhere.

use crate::error::{Error, Result};
use crate::lock::Attempt;

/// Waiting for a board opened exclusively, on macOS, kept between attempts.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) struct Seize<D> {
    /// None while the board re-enumerates, e.g. reset by the holder.
    device: Option<D>,
    /// Once it was held elsewhere, the holder may have reset it.
    may_reset: bool,
    busy: &'static str,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl<D> Seize<D> {
    pub(super) fn new(device: D, busy: &'static str) -> Self {
        Seize {
            device: Some(device),
            may_reset: false,
            busy,
        }
    }

    pub(super) fn attempt<T>(
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
pub(super) struct NodeWait {
    /// Once the board is missing from the list, only its node is tried.
    held: Option<(String, std::path::PathBuf)>,
    /// Once it was gone, e.g. reset by the holder, it may be coming back.
    gone: bool,
}

#[cfg_attr(not(target_os = "freebsd"), allow(dead_code))]
impl NodeWait {
    pub(super) fn attempt<D, T>(
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};

    use async_hid::HidError;

    use super::{refused_as_busy, NodeWait, Seize};
    use crate::error::Error;
    use crate::lock::Attempt;

    fn os_error(kind: std::io::ErrorKind) -> HidError {
        HidError::Other(Box::new(std::io::Error::from(kind)))
    }

    #[test]
    fn only_busy_from_the_transport_counts_as_a_board_in_use() {
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
