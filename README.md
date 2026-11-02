# ykur-rs

Control application and Rust library for the Yepkit YKUR USB relay board: one on-board relay and
four relay driver ports, switched over USB HID. An independent implementation written against
Yepkit's datasheet and checked against a board, not derived from Yepkit's `ykurcmd`.

**For macOS on Apple silicon and Linux on aarch64** (a Raspberry Pi). Both are built and tested
in CI and run against a board, see [Checked against the
hardware](#checked-against-the-hardware). The source refuses to compile anywhere else. The HID
access goes through the `async-hid` crate, so a Rust toolchain is the whole requirement.

## Building

```
cargo install --git https://github.com/gerhard-schuster/ykur-rs --locked
```

### Linux

The board is a file, `/dev/hidrawN`, which belongs to root until a udev rule hands it to a
group. ykurctrl reaches the board only through that node; this rule gives it to `plugdev`:

```
# /etc/udev/rules.d/60-ykur.rules
KERNEL=="hidraw*", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="f1cb", MODE="0660", GROUP="plugdev"
```

Then load it, make the user a member of the group, and replug the board:

```
sudo udevadm control --reload-rules
sudo usermod -aG plugdev "$USER"      # takes effect at the next login
```

Without the rule, every command that opens the board fails and names the rule file.

## Using it

```
ykurctrl [--no-wait] [-s SERIAL] -u PORT     switch on
ykurctrl [--no-wait] [-s SERIAL] -d PORT     switch off
ykurctrl [--no-wait] [-s SERIAL] -g PORT     print whether it is on
ykurctrl [--no-wait] [-s SERIAL] -v          print the firmware version of the board
ykurctrl -l                                  list the attached boards
```

PORT is `1` to `4` for the relay driver ports, `r` for the on-board relay, `a` for all four
ports (the relay is left alone). `-s` picks a board by serial number when several are attached;
without it the first one is used. The switches are those of Yepkit's `ykurcmd`, including
`a` and `-l`, which its manual does not list, with the same meaning; `-g a` is an addition.

Every switching command is confirmed by reading the state back; a target that did not switch is
an error.

The exit status tells a script what to do next; the values follow sysexits(3). The codes are a
stable interface, the wording of the messages is not.

| Code | Meaning | What to do |
|---|---|---|
| `0` | success; also `-l` with no board attached | go on |
| `64` | invalid command line | correct the call |
| `69` | no board, unknown serial number, the board gone, no hidraw node, a node that keeps changing, the HID stack cannot list devices; **the board reported an error**: status `AA`, an unknown error (`00`), an output that did not switch | stop, check the board and its connection |
| `74` | no answer within the timeout, a truncated report, a transport error, output failed | retry |
| `75` | the board is in use (waited 5 seconds, or `--no-wait`) | retry later |
| `76` | an answer that does not fit the request: wrong echo, undocumented value | retry; if it recurs, check the board |
| `77` | access denied (on Linux the udev rule is missing) | fix the setup |
| `101` | a bug in this program (a Rust panic) | report it |

Every command of `ykurctrl` may be repeated: switching sets a state, the others only read.
After 74 or 76 the board may have carried out the command before the answer was lost, which a
repeat does not change. sysexits(3) has no code for a device that reports an error; such an
answer is valid, not a protocol violation (`76`), so it gets the catch-all `69`. `77` is meant
for permissions above the file system; a denied device node is a file system matter, but its
cause is the setup, and that is what the code says.

The same codes as `ykushctrl` of [ykush-rs](https://github.com/gerhard-schuster/ykush-rs).

On Linux a second open is not refused, and each answer of the board reaches every open handle,
so two invocations at once could read each other's answers. There they take turns: an
invocation takes an advisory `flock` on the board's hidraw node before it opens the board, and a
second one waits for it, up to five seconds, then fails with "still not available" (exit 75). With
`--no-wait` it fails at once instead. The kernel drops the lock when the process ends, so there
is no lock file to clean up. The lock orders every `ykurctrl` and every program built on the
library; a tool that opens the node without asking is not held back.

On macOS the board is opened exclusively (seized, `Access::Exclusive` of the transport, from
sidit77/async-hid#55, pinned until it is released): while one handle holds it, every other open
fails, in this process and in any other, and is repeated up to five seconds like on Linux; the
message then reads `another program holds it exclusively`, since the system does not say who.
The seize also holds back programs that do not know about this one.

Measured on 2026-11-02 on a Mac (Apple silicon) with board `YK22170`, release builds of the
commit before the exclusive open and of the exclusive open:

| Run | Shared open (before) | Exclusive open |
|---|---|---|
| two loops reading ports 1 and 2 at the same time, 2 x 500 | 500 failed, each an answer meant for the other loop (exit 76) | none failed |
| the same, 2 x 2540 | - | none failed in 5080 runs |
| the same with `--no-wait`, 2 x 300 | - | 300 succeeded, 300 found the board in use (exit 75), nothing else |

## USB control protocol

From the YKUR datasheet rev. 1.2.1, section 4. The board is a HID device with VID `0x04D8`
and PID `0xF1CB`; every command and answer is a 64 byte report, of which the first bytes count.

| Command | Sent | Answer per datasheet |
|---|---|---|
| Switch on | `01 target` | `01 target status` |
| Switch off | `02 target` | `02 target status` |
| Read state | `03 output` | `03 output state` |
| Firmware version | `20 01` | `20 01 major minor patch` |

Targets: `01` to `04` the ports, `11` the relay, `0A` the four ports, `AA` everything. A state
query takes a single output. Status `FF` means ok and `AA` error; state `00` off, `01` on, `AA`
error. An answer whose first byte is `00` reports an unknown error.

### Where the board differs from the datasheet

Measured on 2026-10-31 against board `YK22170`, firmware 1.2.1, in two runs:

| Command | Datasheet | Board |
|---|---|---|
| Switch on | `01 target FF` | `01 target 00` |
| Switch off | `02 target FF` | `03 target 00`, the format of a state answer |
| Switch off `0A` or `AA` | `02 target FF` | `03 target 02` |
| Read state of `0A` or `AA` | not documented | `03 target 02` |

Switching itself does what the datasheet says, for every target. So `ykurctrl` accepts both
forms of the echo, treats only status `AA` as a failure and confirms the outcome by reading the
state of each output, which the board answers as documented. Yepkit's `ykurcmd` reads the
answer and discards it, so it does not see the difference.

## Tests

```
cargo test                                  # 80 tests (85 on Linux) and a doctest
cargo test -- --ignored --test-threads=1    # 7 more, with a board attached
```

The hardware tests read the firmware version, switch every output on and off one by one, the
four ports at once and everything at once, and put the outputs back the way they found them.
Run them only with nothing attached to the outputs that must not switch. Five tests run only on
Linux and need no board: four take a real `flock` on a file in the temporary directory, the
fifth checks that a missing node fails at once.

## Coverage

Line coverage with `-C instrument-coverage`, measured on 2026-10-31 on the Raspberry Pi (Linux
on aarch64) at commit 44d21e9, which added taking turns at a board on Linux; later changes are
not in these numbers:

| Run | Lines (`llvm-cov report`) | Missed |
|---|---|---|
| `cargo test` only, no board needed | 85.90 % | 176 of 1248 |
| `cargo test` and the hardware run, board `YK22170` attached | 94.95 % | 63 of 1248 |

Line by line, `llvm-cov show` lists 53 unreached lines for the run with the board: 45 in the
library and the binary, 8 in tests. `llvm-cov report` counts 10 more, 8 in `lock.rs` and 2 in
`device.rs`. For `lock.rs` the generic functions are counted per instantiation; for the 2 lines
in `device.rs` the cause is UNKNOWN. The 45 lines are:

| Place | Why it is not reached |
|---|---|
| `device.rs` - `Error::NoResponse` and discarding stale answers | needs a board that goes quiet for five seconds |
| `device.rs` - a failing read | needs the board to report a read error |
| `device.rs` - a truncated read | needs a board that delivers only part of a report |
| `device.rs` - an open refused for a reason other than an operating system error | needs the transport to fail the open itself |
| `device.rs` - a board without a hidraw node in sysfs | needs a transport that does not report the sysfs directory, or a `uevent` without `DEVNAME` |
| `lock.rs` - taking the lock fails for a reason other than a board in use | needs the hidraw node to vanish or refuse opening between being looked up and being locked |
| `lock.rs` - permission denied on the hidraw node when taking the lock | the Pi has the udev rule in place |
| `lock.rs` - `flock` interrupted by a signal, or failing otherwise | needs the kernel to fail the call |
| `error.rs` - conversion of an output error | needs standard output to fail |
| `main.rs` - `-l`, `--no-wait` and a command that opens a board, run in the binary | the binary's own tests do not list boards or open one |
| `main.rs` - `unreachable!()` | help, version and listing are handled before a board is opened |
| `ykur.rs` - the error text naming the four ports or all outputs | needs the board to report an error for those targets |

The 8 lines in tests are failure arms of assertions, the branch for macOS in a hardware test (since
removed), and
the success path of a test helper whose command is meant to fail.
Waiting, giving up and `--no-wait` were also checked by hand on the Pi against `flock` from
util-linux holding the board's hidraw node.

Reproducing it needs the `llvm-tools` component of the toolchain:

```
export RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="/tmp/yk/%p-%m.profraw"
cargo test && cargo test -- --ignored --test-threads=1
llvm-profdata merge -sparse /tmp/yk/*.profraw -o /tmp/yk/yk.profdata
llvm-cov report -object target/debug/deps/<every test binary> \
                -object target/debug/ykurctrl \
                -instr-profile=/tmp/yk/yk.profdata \
                -ignore-filename-regex='(\.cargo/|\.rustup/|/rustc/|/tests/)'
```

## Checked against the hardware

Board `YK22170`, firmware 1.2.1, nothing attached to its outputs, on 2026-10-31:

These runs predate the exit codes: where they say "exits 1", today's program exits with `77`
for a denied open.

On 2026-11-02 the board did not appear on the bus when the YKUSH3 port it hangs on was first
switched on, although it had power; after switching the port off and on again it did. Why is
UNKNOWN; it was not seen on the Raspberry Pi.

| System | What was run | Result |
|---|---|---|
| macOS on Apple silicon, before the exclusive open | the hardware tests; `-l`, `-v`, `-g`, `-u` and `-d` on every port, `r` and `a`; `-s` | all pass |
| macOS on Apple silicon, 2026-11-02, with the exclusive open, the board on port 3 of a YKUSH3 | the hardware tests; `-u`/`-d` on `r` and `a`; exit status of a usage error and an unknown serial number | all pass; `64` and `69` |
| Raspberry Pi 4 Model B, Debian 13, kernel 6.18 (aarch64) | the same, with the udev rule from [Linux](#linux) | all pass |
| Raspberry Pi, without the udev rule | `-l`, then `-g a` | `-l` lists the board; `-g a` exits 1 and names the rule file |

## Where it differs from ykurcmd

Compared with Yepkit's `ykurcmd` (GitHub, commit ae4a095), from its source:

| | `ykurcmd` | `ykurctrl` |
|---|---|---|
| Answer to a switching command | read and discarded | echo checked, then the state read back; a target that did not switch is an error |
| Exit code | 0, whatever happened | 0 on success, otherwise by what to do next, see [Using it](#using-it) |
| No arguments | prints the help, exit 0 | usage error, exit 64 |
| `-g a` | "Unable to get status" | the state of the four ports |
| `-g` output | `Port 1 is ON` | `Port 1: on` |
| `-v` output | `Firmware Version: Rev.1.2.1` | `Firmware 1.2.1` |
| `-l` output | `Attached YKUR Boards` and a list | `YKUR boards on this host:` and a numbered list |

The switches themselves, `-s`, `-u`, `-d`, `-g`, `-v` and `-l`, and the ports `1` to `4`, `r`
and `a` mean the same in both. Several boards without `-s`: both use the first one found.

## Sources

The vendor documents are not kept here; they belong to Yepkit. See [doc/README.md](doc/README.md).

## License

Apache-2.0, see [LICENSE](LICENSE). [NOTICE](NOTICE) says where the protocol comes from and
how this program relates to Yepkit's `ykurcmd`; [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)
lists the crates in the binary with their licenses. How to report a vulnerability, and what
is in scope, is in [SECURITY.md](SECURITY.md).
