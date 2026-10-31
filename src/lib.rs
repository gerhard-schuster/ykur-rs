// SPDX-License-Identifier: Apache-2.0
//! Control library for the Yepkit YKUR USB relay board.
//!
//! An independent implementation of the YKUR USB control protocol, written
//! against Yepkit's datasheet and checked against a board. The `ykurctl`
//! binary is a command line front end; a Rust program can drive the board
//! directly:
//!
//! ```no_run
//! use ykur::{Output, Target, Ykur};
//!
//! # fn main() -> ykur::Result<()> {
//! let board = Ykur::open(None)?;
//! board.switch(Target::One(Output::Relay), true)?;
//! println!("{}", board.state(Output::Relay)?);
//! # Ok(())
//! # }
//! ```

// Built and run against a board on macOS on Apple silicon and on Linux on
// aarch64 (a Raspberry Pi); elsewhere it refuses to compile rather than produce
// a binary nobody has tried.
#[cfg(not(all(target_arch = "aarch64", any(target_os = "macos", target_os = "linux"))))]
compile_error!("ykur is for macOS on Apple silicon and for Linux on aarch64");

mod device;
mod error;
mod sanitize;
mod ykur;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

pub use crate::device::{list, Board, Report, Transport, PRODUCT_ID, REPORT_SIZE, VENDOR_ID};
pub use crate::error::{Error, Result};
pub use crate::sanitize::sanitize;
pub use crate::ykur::{Output, Target, Version, Ykur};
