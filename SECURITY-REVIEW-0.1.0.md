# Security review - 0.1.0

| | |
|---|---|
| Subject | `ykurctrl` 0.1.0, command line and library `ykur` |
| State reviewed | `main` at 4367ea2 ("Say which platform each concurrency statement covers") |
| Transport | `async-hid` 0.6.0 from the fork gerhard-schuster/async-hid, revision 1d1a2bf422219b1332f1a46ec4b9b1bc1b29e08e, read at that revision |
| Date | 2026-11-02 |
| Method | code read by the categories below, the hidraw and IOKit backends of the transport read at the pinned revision, the checks below run, `sanitize()` probed with a list of characters |
| Who | a review without the author's working context, read only; not an outside party, so a record of what was examined, not an independent audit |

No board was attached during the review. What a finding says about the board is marked
UNKNOWN where only the board can tell.

## Checks run

| Command | Exit | Result |
|---|---|---|
| `cargo test --locked -q` (macOS, aarch64) | 0 | 80 tests and a doctest pass, 7 ignored (hardware) |
| `cargo clippy --all-targets --locked -- -D warnings` (macOS) | 0 | no warning |
| the same with `--target aarch64-unknown-linux-gnu` | 0 | no warning, `lock.rs` included |
| `cargo fmt --check` | 0 | clean |
| `cargo deny --all-features check` (cargo-deny 0.20.2, advisory database of 2026-11-02) | 0 | advisories, bans, licenses and sources ok |
| search for `unsafe` in `src/` and `tests/` | - | none |
| search for `unwrap`, `expect`, `panic!`, `unreachable!` in `src/` | - | outside tests only an `unreachable!` in `main.rs` that `run()` guards, and an `expect` in `fake.rs` behind the `fake` feature |
| indexing in `src/` | - | constant indices into the fixed 64 byte report only; no index comes from the device |

## Attack surface examined

| Area | What was found |
|---|---|
| Parsing the answers | a fixed 64 byte report, zeroed for every exchange, read at constant indices; a short report is refused; every command checks the echo. See M1 and L1 |
| Report sizes from the device | Linux: hidraw copies at most the buffer. macOS: the transport copies `min(report, buffer)`. A longer report is cut, a shorter one refused |
| Device text on the terminal | only the serial number, sanitised, and the firmware version as numbers. Product and manufacturer strings are not printed. See B1 |
| Command line | argv is sanitised before parsing; a fixed grammar, no format strings from input, no process spawned; every error has an exit status, without a catch-all |
| Locking on Linux | `flock` on `/dev/hidrawN`; the node name from the sysfs `uevent` is taken only as `hidraw` and digits; checked again after locking; the total wait is bounded. See B4 |
| Locking on macOS | exclusive open, repeated only for `Busy` and only within the wait |
| `unsafe` | none in this repository; in the transport, 3 places in hidraw (one reached) and 31 in the IOKit backend. See B5, B6 |
| Supply chain and CI | see below |

## Findings

### MEDIUM M1 - an answer from an earlier invocation can be read as the first answer of the next one

Every handle started out trusting its queue. The queue was emptied only after a read had timed
out on the same handle. On Linux the kernel polls the board's interrupt endpoint only while the
node is open (`usbhid_open`/`usbhid_close`, from knowledge of the kernel source, not looked up:
MISSING_SOURCE). An answer the board has ready after the node was closed stays in the endpoint,
and the next invocation would get it first. Whether the YKUR firmware keeps an answer that was
never collected is UNKNOWN.

How it comes about: an invocation is ended between its write and its read (Ctrl-C, a `timeout`
around it in a script, a killed CI job), or the board answers after more than five seconds.

What follows, step by step:

1. Invocation A, `-g 1`, is killed after the write. Its answer `03 01 01` stays behind.
2. Invocation B, `-d 1`, sends `02 01` and reads `03 01 01`. Since firmware 1.2.1 answers a
   switch off in the format of a state answer, that passes as the echo.
3. B's read-back sends `03 01` and reads the real answer to the switch off, `03 01 00`, as
   "off". The confirmation rests on the answer to the switching command, not on a state read.
4. The answer to B's state query arrives after B has ended and waits for the next invocation.
5. Invocation C, `-g 1`, prints the state from before the last command, with exit 0.

So the command line can report a stale state as current, and the read-back after switching an
output off can confirm itself. On macOS the system probably polls independently of the
programs that have the device open; UNKNOWN.

### LOW L1 - the library got back in step only after a timeout, and only for 50 ms

Only a timeout marked the queue as suspect. A read error, a short report or an answer that
does not fit the request (`Error::Device`) left it as it was, and the drain after a timeout
waited only 50 ms. A program that keeps a `Ykur` open over several commands could then be one
answer behind for good, and repeated identical commands, polling a state for example, would
return the previous value without an error. The command line reaches this only through M1,
since it ends after an error.

### LOW L2 - the write had no timeout

The read raced a timer, the write did not. On macOS the transport writes with
`IOHIDDeviceSetReport`, which takes no timeout; whether IOKit bounds it is UNKNOWN. A board
that refuses its OUT endpoint forever would hold the invocation forever, and with the exclusive
open every other invocation would fail with exit 75 meanwhile. SECURITY.md said that an
invocation waits at most five seconds for the board, which held for the wait to get the board,
not for the time after that.

### LOW L3 - boards with the same serial number are merged silently

Boards are merged by sanitised serial number, meant for one board with several HID interfaces.
Two boards with the same serial number, or both without one, or serial numbers that differ only
in replaced characters, are listed as one, and `-s` picks whichever the system enumerates
first, with exit 0. A device that copies the serial number of another one, which takes physical
access, can take its place.

## Observations below the reporting bar

- **B1** `sanitize()` replaced control characters and a list of layout characters, but let
  others through: tag characters, U+2028, U+2029, U+180E, U+FFF9, U+3164, U+034F, U+0336 and
  look-alike letters such as Cyrillic U+0417. The test name "two serial numbers cannot be made
  to look alike" promised more than that.
- **B2** With several boards and no `-s`, the first one enumerated is used, as in `ykurcmd`;
  the order is not defined.
- **B3** The library accepts `Duration::MAX` as the wait and then waits without limit, on
  purpose; the command line passes 0 or 5 seconds only.
- **B4** Linux lock: between the check after locking and the open, the transport reads `uevent`
  again; a replug in exactly that window with the same `hidrawN` would leave the lock on the
  old node. `flock` works per inode, so an instance in a container with its own `/dev` and one
  on the host do not exclude each other. The node path is built from `/dev` and `hidraw` plus
  digits only; no way to point it elsewhere was found.
- **B5** Transport on macOS: enumeration walks every HID device of the system and panics if
  IOKit returns no device set, unwraps number properties, and transmutes an array of
  dictionaries without checking the element types. The properties come from IOKit; a virtual
  HID device sets them itself but needs an entitlement. At worst exit 101 or undefined
  behaviour, for an actor who is privileged already. Whether IOKit checks the types:
  MISSING_SOURCE.
- **B6** Transport on macOS, input callback: it trusts IOKit that the reported length does not
  exceed the registered buffer. Cancelling a read is safe: the read copies from a queue the
  library owns, and dropping waits for the cancellation before freeing.
- **B7** The hidraw descriptor parser of the transport reads the descriptors of every hidraw
  device; no panic found, every item advances at least one byte, errors of a single device are
  dropped during enumeration.
- **B8** The udev rule in the README gives the group `plugdev` access; every member can switch
  and hold the lock, as SECURITY.md says. A group of its own would be narrower.
- **B9** No `#![forbid(unsafe_code)]`; it would change nothing today but keep it that way.

## Supply chain

| Point | Finding |
|---|---|
| Graph | 50 packages in `Cargo.lock`, Windows crates included; about 33 built for macOS on aarch64, about 24 for Linux on aarch64 |
| Build scripts, proc macros | none in this repository; build scripts in async-io, crossbeam-utils, libc, memoffset, nix, objc2 and rustix; proc macros only for Windows, not built |
| Git source | the only one; `allow-git` names exactly the fork, `unknown-git = "deny"`; the revision is in the lock file in full |
| Availability of the pin | the revision is reachable only from the branch `open-options` of the fork. If that branch goes, building depends on GitHub still serving the object. No integrity problem, a reproducibility risk |
| Features | `fake` adds no dependency; `async-hid` with its default features; `rustix` with `fs` only, Linux only |
| Licences | Apache-2.0, MIT, Unicode-3.0, as `deny.toml` explains |
| CI permissions | `contents: read` in both workflows, `persist-credentials: false`, no `pull_request_target`, no secrets; a pull request from a fork runs with a read token |
| Action pins | commit hashes, checked against the tags they name |
| Gaps | `cargo deny` runs on pushes that change the manifests and on a schedule, not on pull requests; the toolchain of the test and lint jobs is not pinned; clippy is called without `-- -D warnings` and relies on `RUSTFLAGS` |

## Checked against SECURITY.md

| Statement | Assessment |
|---|---|
| answers from the device are not trusted, in scope | right; M1 and L1 fall under it |
| any local process with access to the device can switch (out of scope) | right |
| "waits at most five seconds for the board" | right for the wait to get the board only; see L2 |
| Linux: an advisory lock, no protection against `ykurcmd` | right, the code agrees |
| macOS: exclusive open, a second invocation waits | the code agrees; the measurements were not repeated |

## Scope

Read: every tracked file under `src/`, `tests/cli.rs` (skimmed), the head of `tests/encoding.rs`,
`Cargo.toml`, the sources in `Cargo.lock`, `deny.toml`, both workflows, `SECURITY.md` and the
relevant parts of the README; in the transport the hidraw and IOKit backends, enumeration and
the utilities at the pinned revision.

Not examined: the behaviour of the board (no board attached, hence UNKNOWN in M1 and L2); the
kernel source on polling and write timeouts (MISSING_SOURCE); what IOKit guarantees (B5, B6);
tests on Linux, which were only checked with clippy; the Windows and FreeBSD backends of the
transport, not reached; transitive crates other than `async-hid`; the measurements in the README
and SECURITY.md.

A review without a HIGH finding is no proof of correctness. It says only that nothing more
serious was found in the areas examined.

## Addendum - what was changed afterwards

| Finding | Change |
|---|---|
| M1, L1 | A handle starts out of step, and every exchange that fails - timeout, read error, short report - or gets an answer that does not fit puts it out of step again. Out of step, the next exchange first asks for the firmware version (datasheet 4.2.1, read only) and drops every report before its answer, at most 16 and within five seconds plus the quiet described next; the board answers in order, so whatever was left behind comes first. Since a version answer may itself have been left behind, by a process killed during the query, the queue then has to stay quiet for 50 ms, and another report means the wait goes on. An answer slower than that is read by the next command and fails its echo check (exit 76), unless that command is a version query too. Tests drive the exchange through a stand-in for the transport: an answer left behind, a late answer, a version answer left behind, every kind of failed exchange, a short report and a read error while getting in step, a board that does not answer the query, one that never gets to it |
| L2 | The write races a five second timer like the read. On macOS that bounds it; on Linux the transport's write call runs inside the kernel and returns only when the kernel's USB transfer does, so there the kernel's timeout bounds it. SECURITY.md says so |
| L3 | Not changed; listed under known limitations in SECURITY.md |
| B1 | `sanitize()` lets printable ASCII through and U+FFFD, which marks a byte of argv that was not UTF-8; everything else becomes `?`. A test covers the characters above |

On 2026-11-02 the changed exchange ran against board `YK22170` on macOS: the hardware tests
pass, switching from the command line is confirmed, and two parallel loops of 200 invocations did
not fail. M1 was tried there: a separate program wrote a state query and exited without reading,
then `-g 2` ran, with the build before the change and after it. Both answered correctly, so on
macOS the answer left behind did not reach the next invocation in these runs. On Linux,
where the review expects it, it has not been tried yet.
