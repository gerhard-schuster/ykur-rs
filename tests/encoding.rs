// SPDX-License-Identifier: Apache-2.0
//! Every text file in this repository is valid UTF-8 and stays inside Latin-1.
//!
//! The rule is deliberately not "ASCII only". German text needs its umlauts and
//! those live in Latin-1. What it keeps out is typography: em and en dashes,
//! curly quotes, the ellipsis character, arrows, box drawing. A word processor
//! or a careless paste brings those in, and they are invisible in a diff until
//! a terminal or a toolchain trips over them.
//!
//! Three Latin-1 characters are rejected anyway, because they are invisible or
//! steer a terminal: the C1 controls, the no-break space and the soft hyphen.
//! They are the same characters `sanitize()` keeps out of the program's output.
//!
//! Written as a test rather than a script so it needs nothing the build does
//! not already need, and so the rule itself can be tested. The file list comes
//! from `git ls-files`, which is the only honest definition of what belongs to
//! the repository: a tree walk also finds editor droppings and locally excluded
//! files that are nobody's business here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A byte-level match inside compressed data carries no meaning, so these are
/// skipped rather than reported. None are in the tree today; the list is here
/// for the day one is.
const BINARY: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "gif", "ico", "zip", "gz", "tar", "o", "a", "so", "dylib", "dll",
    "lib", "exe", "bin", "woff", "woff2", "ttf",
];

/// Why a file does not pass.
#[derive(Debug, PartialEq, Eq)]
enum Reject {
    /// Not valid UTF-8 at all, with the byte offset where decoding stopped.
    NotUtf8 { at: usize },
    /// A character above U+00FF, which Latin-1 cannot hold.
    Outside { line: usize, col: usize, ch: char },
    /// Inside Latin-1, but invisible or a terminal control.
    Invisible {
        line: usize,
        col: usize,
        ch: char,
        what: &'static str,
    },
}

impl Reject {
    fn describe(&self, path: &Path) -> String {
        match self {
            Reject::NotUtf8 { at } => format!("{}: not valid UTF-8, byte {at}", path.display()),
            Reject::Outside { line, col, ch } => format!(
                "{}:{line}:{col}: U+{:04X} outside Latin-1",
                path.display(),
                *ch as u32
            ),
            Reject::Invisible {
                line,
                col,
                ch,
                what,
            } => format!(
                "{}:{line}:{col}: U+{:04X} {what}",
                path.display(),
                *ch as u32
            ),
        }
    }
}

/// The rule, on raw bytes. Returns the first thing that does not belong.
fn check(bytes: &[u8]) -> Option<Reject> {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            return Some(Reject::NotUtf8 {
                at: e.valid_up_to(),
            })
        }
    };

    let (mut line, mut col) = (1usize, 0usize);
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
            continue;
        }
        col += 1;

        let what = match ch as u32 {
            0x00A0 => "no-break space",
            0x00AD => "soft hyphen",
            0x0080..=0x009F => "C1 control",
            0x0100.. => return Some(Reject::Outside { line, col, ch }),
            _ => continue,
        };
        return Some(Reject::Invisible {
            line,
            col,
            ch,
            what,
        });
    }
    None
}

/// The files git tracks, minus the binary extensions above.
///
/// Returns `None` when git cannot answer, so an unpacked source tree without a
/// checkout skips the sweep instead of failing it. The rule tests below do not
/// depend on this.
fn tracked_files(root: &Path) -> Option<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| root.join(String::from_utf8_lossy(s).as_ref()))
            .filter(|p| {
                let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase());
                !ext.is_some_and(|e| BINARY.contains(&e.as_str()))
            })
            .collect(),
    )
}

#[test]
fn every_file_is_valid_utf8_within_latin1() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(files) = tracked_files(root) else {
        eprintln!("no git checkout here, skipping the sweep");
        return;
    };
    assert!(files.len() > 10, "git listed only {} files", files.len());

    let found: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let bytes = fs::read(p).ok()?;
            let rejected = check(&bytes)?;
            let shown = p.strip_prefix(root).unwrap_or(p);
            Some(rejected.describe(shown))
        })
        .collect();

    assert!(
        found.is_empty(),
        "{} of {} files hold characters that do not belong:\n  {}",
        found.len(),
        files.len(),
        found.join("\n  ")
    );
}

#[test]
fn ascii_and_umlauts_are_accepted() {
    assert_eq!(check(b"plain ascii\n"), None);
    assert_eq!(
        check("Umlaute \u{e4}\u{f6}\u{fc}\u{c4}\u{d6}\u{dc}\u{df}\n".as_bytes()),
        None
    );
    // The highest character Latin-1 holds.
    assert_eq!(check("\u{ff}\n".as_bytes()), None);
}

#[test]
fn typography_is_rejected_with_its_position() {
    let cases = [
        ("a \u{2014} b", 0x2014), // em dash
        ("a \u{2013} b", 0x2013), // en dash
        ("a \u{2019} b", 0x2019), // right single quote
        ("a \u{2026} b", 0x2026), // ellipsis
        ("a \u{2192} b", 0x2192), // arrow
        ("a \u{2500} b", 0x2500), // box drawing
    ];
    for (text, codepoint) in cases {
        let got = check(text.as_bytes());
        assert_eq!(
            got,
            Some(Reject::Outside {
                line: 1,
                col: 3,
                ch: char::from_u32(codepoint).unwrap()
            }),
            "for U+{codepoint:04X}"
        );
    }
}

#[test]
fn invisible_latin1_characters_are_rejected() {
    for (ch, what) in [
        ('\u{a0}', "no-break space"),
        ('\u{ad}', "soft hyphen"),
        ('\u{9b}', "C1 control"),
    ] {
        let text = format!("a{ch}b");
        assert_eq!(
            check(text.as_bytes()),
            Some(Reject::Invisible {
                line: 1,
                col: 2,
                ch,
                what
            }),
            "for U+{:04X}",
            ch as u32
        );
    }
}

#[test]
fn broken_utf8_is_rejected() {
    // 0xFF never appears in valid UTF-8.
    assert_eq!(check(b"ok\xff"), Some(Reject::NotUtf8 { at: 2 }));
    // A lead byte with its continuation missing.
    assert_eq!(check(b"ok\xe2\x80"), Some(Reject::NotUtf8 { at: 2 }));
}

#[test]
fn the_position_counts_lines_and_columns() {
    let got = check("first\nsecond \u{2014}\n".as_bytes());
    assert_eq!(
        got,
        Some(Reject::Outside {
            line: 2,
            col: 8,
            ch: '\u{2014}'
        })
    );
}
