// SPDX-License-Identifier: Apache-2.0
//! Test double for the HID transport.
//!
//! Answers from a prepared queue and records what was sent, so the protocol
//! layer, and code built on this library, can be exercised without hardware.
//! The methods panic on misuse, which is right for a test helper; that is why
//! it sits behind the `fake` feature.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use crate::device::{report, Report, Transport};
use crate::error::{Error, Result};

#[derive(Default)]
pub struct FakeBoard {
    sent: RefCell<Vec<Report>>,
    answers: RefCell<VecDeque<Result<Report>>>,
    desyncs: Cell<usize>,
}

impl FakeBoard {
    /// A board with no answers prepared; any read fails the test.
    pub fn new() -> Self {
        FakeBoard::default()
    }

    /// Queues `payload`, zero padded, as the answer to the next request.
    pub fn then(self, payload: &[u8]) -> Self {
        self.answers.borrow_mut().push_back(Ok(report(payload)));
        self
    }

    /// Queues a transport failure as the outcome of the next request.
    pub fn then_fail(self, error: Error) -> Self {
        self.answers.borrow_mut().push_back(Err(error));
        self
    }

    /// The first `len` bytes of every report sent, in order.
    pub fn sent(&self, len: usize) -> Vec<Vec<u8>> {
        self.sent
            .borrow()
            .iter()
            .map(|r| r[..len].to_vec())
            .collect()
    }

    /// How often the protocol layer said the queue cannot be trusted.
    pub fn desyncs(&self) -> usize {
        self.desyncs.get()
    }

    /// Whether every prepared answer was used.
    pub fn exhausted(&self) -> bool {
        self.answers.borrow().is_empty()
    }
}

impl Transport for FakeBoard {
    fn transfer(&self, out: &Report) -> Result<Report> {
        self.sent.borrow_mut().push(*out);
        self.answers
            .borrow_mut()
            .pop_front()
            .expect("command read from the board although no answer was prepared")
    }

    fn desync(&self) {
        self.desyncs.set(self.desyncs.get() + 1);
    }
}
