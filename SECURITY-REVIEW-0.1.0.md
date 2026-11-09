# Security review - 0.1.0

| | |
|---|---|
| Subject | `ykurctrl` 0.1.0, command line and library `ykur` |
| State reviewed | based on `main` at 8bc38f5 ("Test the waiting on macOS and FreeBSD against fakes"); the update also describes the fixes that followed it, in the change that added this update |
| First review | 2026-11-02 at 4367ea2 ("Say which platform each concurrency statement covers") |
| This update | 2026-11-07; re-read of `src/`, workflows, `SECURITY.md`, `deny.toml` and the pinned transport revision against the fixes and the FreeBSD port that landed after the first review |
| Transport | `async-hid` 0.6.0 from the fork gerhard-schuster/async-hid, revision 1d1a2bf422219b1332f1a46ec4b9b1bc1b29e08e, unchanged since the first review |
| Method | original: code read by the categories below, the hidraw and IOKit backends of the transport read at the pinned revision, the checks below run, `sanitize()` probed with a list of characters. Update: code re-read for the remediations, FreeBSD paths and remaining gaps; checks re-run on macOS aarch64 |
| Who | first review: without the author's working context, read only; not an outside party, so a record of what was examined, not an independent audit. Update: committed together with the fixes; whether it was written without the author's context is not recorded (UNKNOWN) |

No board was attached during the first review or this update. What a finding says about
the board is marked UNKNOWN where only the board can tell. Hardware notes below come from
later measurements recorded in the README, CHANGELOG and the hardware notes on the remediations.

Corrected on 2026-11-09 after an independent review of the branch: the state the update
describes, the FreeBSD backend of the transport, the bound on writes, the scope of the update and
the state of the CI check. The FreeBSD source was read then too. These corrections change the
record, not its findings, and are not a new review. The `ENXIO` mapping for FreeBSD is a code
change made on the same day.

## Current status (2026-11-07)

| Finding | Status |
|---|---|
| MEDIUM M1 - stale answer taken as the next answer | **Fixed in code** (version-query resync with quiet window). Empirically tried on macOS (stale answer did not appear). **Not yet tried on Linux**, where the review expects the stale endpoint. |
| LOW L1 - get back in step only after a timeout | **Fixed** with M1: every failed or unfitting exchange desyncs; resync before the next transfer |
| LOW L2 - write had no timeout | **Fixed**: write races a five second timer like the read. On Linux the kernel USB timeout still bounds the write inside the syscall |
| LOW L3 - boards with the same serial number are merged | **Open**; documented under known limitations in SECURITY.md |
| B1 - `sanitize()` too narrow | **Fixed**: printable ASCII and U+FFFD only |
| B9 - no `#![forbid(unsafe_code)]` | **Fixed** (`src/lib.rs`, `src/main.rs`) |
| Supply-chain gaps (deny on PRs, unpinned CI toolchain, git-pin availability) | **Fixed except git-pin availability**: `audit.yml` runs on `pull_request`, `ci.yml` pins `RUST_TOOLCHAIN` and runs `clippy -- -D warnings` including `--target aarch64-unknown-freebsd`, and `NOTICE` / `THIRD-PARTY-NOTICES.md` cover FreeBSD |
| Disconnect exit code differs on Linux (`ENODEV` -> 74 vs Unavailable -> 69) | **Fixed**: `ENODEV` (19) in `From<HidError>` now maps to `Error::Unavailable` (exit 69), and since 2026-11-09 so does `ENXIO` (6), what FreeBSD answers after a detach (read in the 15.1 source, not measured) |
| FreeBSD hidraw node name less strictly validated than Linux | **Fixed**: `is_hidraw_name` requires `hidraw` plus ASCII digits on both Linux and FreeBSD |
| FreeBSD / CI FreeBSD target not compiled in CI | **Fixed**: `ci.yml` checks `--target aarch64-unknown-freebsd` in both `lint` and `minimum-supported-rust`. Rust 1.86 has no standard library for that target, so the minimum supported Rust went to 1.98. On 2026-11-09 a CI run of this change passed both steps for that target, on both runners. They compile and lint it; the tests do not run on FreeBSD in CI |

The original finding write-ups are kept below as the record of what was found. The tables of
the first report were updated in place; the report as first written is in git at e031a37. Remediation
detail is in [Remediations after the first review](#remediations-after-the-first-review).

## Checks run

### First review (2026-11-02, at 4367ea2)

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

### Update (2026-11-07, on a tree after 8bc38f5)

The exact tree was not recorded. The results below do not hold at 8bc38f5 itself, which has 106
tests and no `#![forbid(unsafe_code)]`; they describe the fixes applied, with 108 tests, before the
tests added on 2026-11-09.

| Command | Exit | Result |
|---|---|---|
| `cargo test --locked -q` (macOS, aarch64) | 0 | 108 active tests and a doctest pass, 7 ignored (hardware); 113 active under Linux when the five Linux-only unit tests run, a count, not a run |
| `cargo deny --all-features check` (cargo-deny 0.20.2) | 0 | advisories, bans, licenses and sources ok |
| search for `unsafe` in `src/` and `tests/` | - | none (`#![forbid(unsafe_code)]` in `src/lib.rs` and `src/main.rs`) |
| indexing in `src/` | - | unchanged: constant indices into the fixed 64 byte report only |

CI is set up to build and test on `macos-latest` and `ubuntu-24.04-arm`, and both `lint`
(`clippy`) and `minimum-supported-rust` (`cargo check`) also compile-check
`--target aarch64-unknown-freebsd`. On 2026-11-09 a CI run of this change passed all of its jobs.
FreeBSD-specific concurrency state (`NodeWait`) is additionally exercised by unit tests
against fakes on every host.

## Attack surface examined

| Area | What was found (updated) |
|---|---|
| Parsing the answers | a fixed 64 byte report, zeroed for every exchange, read at constant indices; a short report is refused; every command checks the echo. Fresh handles and failed exchanges run a version-query resync before the next transfer. See M1 and L1 |
| Report sizes from the device | Linux: hidraw copies at most the buffer. macOS: the transport copies `min(report, buffer)`. A longer report is cut, a shorter one refused |
| Device text on the terminal | only the serial number, sanitised (printable ASCII and U+FFFD), and the firmware version as numbers. Product and manufacturer strings are not printed. See B1 |
| Command line | argv is sanitised before parsing; a fixed grammar, no format strings from input; every error has an exit status, without a catch-all. On FreeBSD only, `/sbin/sysctl` is spawned during enumeration (fixed argv, no user input in the program name) |
| Locking on Linux | `flock` on `/dev/hidrawN`; the node name from the sysfs `uevent` is taken only as `hidraw` and digits; checked again after locking; the total wait is bounded. See B4 |
| Locking on macOS | exclusive open, repeated only for `Busy` and only within the wait; waiting survives a reset/replug of a held board up to the wait limit |
| Locking on FreeBSD | hidraw allows one open at a time; the open is the turn. A held board is missing from the HID list and is found via `sysctl` on `/dev/hidraw*`. While waiting, only that board's node is reopened. Node names are checked with `is_hidraw_name` (`hidraw` plus ASCII digits), matching Linux |
| Timeouts | every read and every write races five seconds in this program; on Linux the write still returns only when the kernel USB transfer does. On FreeBSD the transport's own comment says the write runs synchronously in the kernel, so the timer does not bound it either; `usbhid_sync_xfer` waits at most `USB_DEFAULT_TIMEOUT`, 5000 ms, for the transfer. An interrupt OUT transfer has no timeout of its own and counts as done once submitted, so a write can return before the board took the report. A second caller waits without a limit of its own for the one in progress (`sys/dev/usb/input/usbhid.c`, `sys/dev/usb/usbdi.h`, releng/15.1; read, not measured; whether the YKUR has an interrupt OUT endpoint was not checked) |
| `unsafe` | none in this repository (`#![forbid(unsafe_code)]`); in the transport, 3 places in hidraw (one reached) and 31 in the IOKit backend. See B5, B6. FreeBSD does not use the hidraw path: it has its own backend `src/backend/freebsdhid/`, with 8 lines containing `unsafe`, not assessed here |
| Supply chain and CI | see below |

## Findings (as of the first review)

Status for each is in [Current status](#current-status-2026-11-07). The text is the
original finding.

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

Statuses in italics are as of this update.

- **B1** `sanitize()` replaced control characters and a list of layout characters, but let
  others through: tag characters, U+2028, U+2029, U+180E, U+FFF9, U+3164, U+034F, U+0336 and
  look-alike letters such as Cyrillic U+0417. The test name "two serial numbers cannot be made
  to look alike" promised more than that. *Fixed: printable ASCII and U+FFFD only.*
- **B2** With several boards and no `-s`, the first one enumerated is used, as in `ykurcmd`;
  the order is not defined on macOS and Linux. *On FreeBSD, listing and the default board follow
  the path order of `/dev/hidraw*`, including a board that is held open.*
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
  and hold the lock, as SECURITY.md says. A group of its own would be narrower. *FreeBSD uses a
  `devfs.rules` entry for `hidraw*` and group `operator`; same breadth for every hidraw node.*
- **B9** No `#![forbid(unsafe_code)]`; it would change nothing today but keep it that way.
  *Fixed: set in `src/lib.rs` and `src/main.rs`.*
- **B10** (update) On FreeBSD, `hidraw_boards` accepted any `/dev` name starting with
  `hidraw` and fed it into fixed `sysctl` keys, whereas Linux required `hidraw` plus digits.
  *Fixed: both Linux (`hidraw_node`) and FreeBSD (`hidraw_boards`) now use `is_hidraw_name`
  (`hidraw` followed by one or more ASCII digits).*
- **B11** (update) After an unplug, macOS tends to surface `NotConnected` /
  `Disconnected` -> `Error::Unavailable` (exit 69), whereas the pinned Linux transport often
  surfaced `ENODEV` as a generic `Hid` error (exit 74). *Fixed: `From<HidError>` in
  `src/error.rs` now maps `ENODEV` (19) inside `HidError::Other` to `Error::Unavailable`
  (exit 69), and since 2026-11-09 `ENXIO` (6), what FreeBSD answers after a detach, likewise.*
- **B12** (update) FreeBSD enumeration runs `/sbin/sysctl -n ...` as a subprocess instead of
  `sysctlbyname(3)`. The binary path is fixed; names come from `/dev` and constructed keys.
  Avoids unsafe FFI; acceptable trade-off, noted for completeness.

## Supply chain

| Point | Finding |
|---|---|
| Graph | 50 packages in `Cargo.lock`, Windows crates included; 29 built for macOS on aarch64, 24 for Linux on aarch64 and 24 for FreeBSD on aarch64 (`errno` instead of `linux-raw-sys`), by `cargo tree --edges normal,build` with this package included |
| Build scripts, proc macros | none in this repository; build scripts in async-io, crossbeam-utils, libc, memoffset, nix, objc2 and rustix; proc macros only for Windows, not built |
| Git source | the only one; `allow-git` names exactly the fork, `unknown-git = "deny"`; the revision is in the lock file in full |
| Availability of the pin | the revision is reachable only from the branch `open-options` of the fork. If that branch goes, building depends on GitHub still serving the object. No integrity problem, a reproducibility risk. Still open; crates.io 0.6.0 with #55 would retire it |
| Features | `fake` adds no dependency; `async-hid` with its default features; `rustix` with `fs` only, Linux only |
| Licences | Apache-2.0, MIT, Unicode-3.0, as `deny.toml` explains. `NOTICE` and `THIRD-PARTY-NOTICES.md` cover macOS, Linux and FreeBSD |
| CI permissions | `contents: read` in both workflows, `persist-credentials: false`, no `pull_request_target`, no secrets; a pull request from a fork runs with a read token |
| Action pins | commit hashes, checked against the tags they name |
| CI & audit coverage | `audit.yml` runs `cargo deny` on pushes, pull requests (touching the manifests), and on a weekly schedule; `ci.yml` pins `RUST_TOOLCHAIN` (included in the cache key), passes `-- -D warnings` to `clippy`, and compile-checks `--target aarch64-unknown-freebsd` in both `lint` and `minimum-supported-rust` |

## Checked against SECURITY.md

| Statement | Assessment |
|---|---|
| answers from the device are not trusted, in scope | right; M1 and L1 fell under it and are addressed in code; Linux hardware confirmation of M1 still missing |
| any local process with access to the device can switch (out of scope) | right |
| read and write give up after five seconds (macOS write in-process; Linux write by the kernel; FreeBSD write by the kernel, read in the source, not measured) | right after L2; matches SECURITY.md |
| Linux: an advisory lock, no protection against `ykurcmd` | right, the code agrees |
| macOS: exclusive open, a second invocation waits | the code agrees; measurements in the README were not repeated for this update |
| FreeBSD: one open at a time; held boards found via sysctl; only that node reopened while waiting | right, the code agrees; measured on FreeBSD 15.1 in a VM (see README / CHANGELOG) |
| known limitation: serial-number merge | right; L3 unchanged |
| known limitation: resync via version query and 50 ms quiet | right; matches the remediation; residual risk if a late answer follows a version query and the next command is also a version query |

## Scope

**First review:** every tracked file under `src/`, `tests/cli.rs` (skimmed), the head of
`tests/encoding.rs`, `Cargo.toml`, the sources in `Cargo.lock`, `deny.toml`, both workflows,
`SECURITY.md` and the relevant parts of the README; in the transport the hidraw and IOKit
backends, enumeration and the utilities at the pinned revision.

**Update:** the same files on the tree described under the checks of the update, with attention
to `device.rs` / `lock.rs` FreeBSD and
wait/reset paths, `sanitize.rs`, error mapping, `SECURITY.md`, README hardware notes, and
findings of an earlier code review, not kept in this repository, that touch the trust boundary or
supply chain.

Not examined: the tests on Linux and FreeBSD, since the checks of the update ran on macOS only;
whether the FreeBSD backend of the transport was read is not recorded (UNKNOWN), so treat it as
not examined; the behaviour of the board during this update (no board attached); the Linux
kernel source on polling and write timeouts (MISSING_SOURCE), while the FreeBSD source on writes
was read for the correction of 2026-11-09; what IOKit guarantees (B5, B6); a full
re-read of transitive crates other than confirming the same `async-hid` pin; the Windows
backend of the transport, not reached; repeating the README/SECURITY.md timing measurements.

A review without a HIGH finding is no proof of correctness. It says only that nothing more
serious was found in the areas examined.

## Remediations after the first review

| Finding | Change |
|---|---|
| M1, L1 | A handle starts out of step, and every exchange that fails - timeout, read error, short report - or gets an answer that does not fit puts it out of step again. Out of step, the next exchange first asks for the firmware version (datasheet 4.2.1, read only) and drops every report before its answer, at most 16 and within five seconds plus the quiet described next; the board answers in order, so whatever was left behind comes first. Since a version answer may itself have been left behind, by a process killed during this very query, the queue then has to stay quiet for 50 ms, and another report means the wait goes on. An answer slower than that is read by the next command and fails its echo check (exit `76`), unless that command is a version query too. Tests drive the exchange through a stand-in for the transport: an answer left behind, a late answer, a version answer left behind, every kind of failed exchange, a short report and a read error while getting in step, a board that does not answer the query, one that never gets to it |
| L2 | The write races a five second timer like the read. On macOS that bounds it; on Linux the transport's write call runs inside the kernel and returns only when the kernel's USB transfer does, so there the kernel's timeout bounds it. SECURITY.md says so |
| L3 | Not changed; listed under known limitations in SECURITY.md |
| B1 | `sanitize()` lets printable ASCII through and U+FFFD, which marks a byte of argv that was not UTF-8; everything else becomes `?`. A test covers the characters above |
| FreeBSD port (after the first review) | Concurrent use via exclusive hidraw open; held boards recovered through `/sbin/sysctl`; wait path reopens only the known node; node names validated with `is_hidraw_name`; fakes cover `NodeWait` on every CI host; `ci.yml` compile-checks `aarch64-unknown-freebsd` in `lint` and `minimum-supported-rust`; `SECURITY.md`, `README.md`, `NOTICE` and `THIRD-PARTY-NOTICES.md` describe it |
| Waiting across reset/unplug (1988e90, 8bc38f5; the `ENODEV` and `ENXIO` mappings came with later fixes) | On macOS and FreeBSD a waiting open keeps trying while the board re-enumerates, up to the wait. Busy messages keep a useful serial where possible. Unplug after open maps `NotConnected`, `Disconnected`, Linux `ENODEV` (19) and since 2026-11-09 FreeBSD `ENXIO` (6) to `Error::Unavailable` (exit 69) - B11 |

### Hardware notes on the remediations

On 2026-11-02 the changed exchange ran against board `YK22170` on macOS: the hardware tests
pass, switching from the command line is confirmed, and two parallel loops of 200 invocations did
not fail. M1 was tried there: a separate program wrote a state query and exited without reading,
then `-g 2` ran, with the build before the change and after it. Both answered correctly, so on
macOS the answer left behind did not reach the next invocation in these runs. On Linux,
where the review expects it, it has not been tried yet.

On 2026-11-03 FreeBSD 15.1 (aarch64) in a VM, board on a YKUSH3 port: hardware tests pass;
parallel `-g` loops of 200 each none failed; a held node makes `--no-wait` exit 75 and a
waiting call proceed when free; `-l` still lists the held board; permission denied exits 77.
Waiting across re-enumeration was measured there with a reset YKUSH3 (about 5 s in the VM).
While an invocation waited, 3000 opens of other devices saw no `EBUSY` failures from reopening
only the held board's node (against 23 with listing on every attempt).

## Remaining work that touches security or trust

These remain open after the 2026-11-07 fixes:

1. Repeat the M1 stale-answer experiment on Linux (Raspberry Pi), before relying on the
   resync alone for that platform.
2. Protect or tag the fork commit used by the `async-hid` pin, or move to crates.io once #55
   ships.
