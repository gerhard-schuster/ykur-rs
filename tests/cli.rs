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
fn a_reader_that_closed_the_pipe_is_not_an_error() {
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    let out = Command::new(env!("CARGO_BIN_EXE_ykurctrl"))
        .arg("-h")
        .stdout(writer)
        .output()
        .expect("the binary should run");

    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
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
    assert_eq!(out.status.code(), Some(64));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("Invalid port 9"));
    assert!(err.contains("Usage: ykurctrl"));
}

#[test]
fn no_arguments_is_a_usage_error() {
    let out = ykurctrl(&[]);
    assert_eq!(out.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("No command given"));
}

#[test]
fn an_unknown_serial_fails_without_touching_any_board() {
    let out = ykurctrl(&["-s", "no-such-board", "-g", "r"]);
    assert_eq!(out.status.code(), Some(69));
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

#[test]
fn the_manual_page_covers_every_option_and_exit_code() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("man/ykurctrl.1");
    let man = std::fs::read_to_string(&path).expect("man/ykurctrl.1 should be readable");

    for needle in [
        ".Dt YKURCTRL 1",
        ".It Fl u Ar PORT",
        ".It Fl d Ar PORT",
        ".It Fl g Ar PORT",
        ".It Fl v",
        ".It Fl l",
        ".It Fl s Ar SERIAL",
        ".It Fl -no-wait",
        ".It Fl -version",
        ".It Fl h , Fl -help",
        ".It Cm 1 .. Cm 4",
        ".It Cm r",
        ".It Cm a\n",
        ".It Cm everything",
        ".It Cm 0\n",
        ".It Cm 64",
        ".It Cm 69",
        ".It Cm 74",
        ".It Cm 75",
        ".It Cm 76",
        ".It Cm 77",
        ".It Cm 101",
    ] {
        assert!(man.contains(needle), "man/ykurctrl.1 is missing {needle:?}");
    }
}

#[test]
fn the_manual_page_describes_every_option_of_the_help() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("man/ykurctrl.1");
    let man = std::fs::read_to_string(&path).expect("man/ykurctrl.1 should be readable");
    let help = ykurctrl(&["-h"]);
    let help = String::from_utf8_lossy(&help.stdout);

    let options: Vec<&str> = help
        .lines()
        .filter(|line| line.starts_with("  -"))
        .filter_map(|line| line.split_whitespace().next())
        .map(|option| option.trim_end_matches(','))
        .collect();
    assert!(options.len() >= 9, "too few options found: {options:?}");

    for option in options {
        let name = option.strip_prefix('-').expect("an option");
        let needle = format!(".It Fl {name}");
        assert!(
            man.contains(&needle),
            "man/ykurctrl.1 is missing {needle:?}"
        );
    }
}
