// SPDX-License-Identifier: Apache-2.0
//! The built binary: exit codes, and what goes to stdout and stderr. None of
//! these needs a board.

use std::process::{Command, Output};

fn ykurctrl(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ykurctrl"))
        .args(args)
        .output()
        .expect("the binary should run")
}

#[test]
fn help_goes_to_stdout_and_succeeds() {
    let out = ykurctrl(&["-h"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Usage: ykurctrl"));
    assert!(out.stderr.is_empty());
}

#[test]
fn the_version_names_the_program() {
    let out = ykurctrl(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("ykurctrl {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn a_usage_error_fails_with_the_reason_and_the_usage_on_stderr() {
    let out = ykurctrl(&["-u", "9"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("Invalid port 9"));
    assert!(err.contains("Usage: ykurctrl"));
}

#[test]
fn no_arguments_is_a_usage_error() {
    let out = ykurctrl(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("No command given"));
}

#[test]
fn an_unknown_serial_fails_without_touching_any_board() {
    let out = ykurctrl(&["-s", "no-such-board", "-g", "r"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "No YKUR board with serial number no-such-board found\n"
    );
}

#[test]
fn terminal_escapes_in_arguments_are_not_echoed() {
    let out = ykurctrl(&["-u", "\u{1b}[31m"]);
    assert!(!String::from_utf8_lossy(&out.stderr).contains('\u{1b}'));
}
