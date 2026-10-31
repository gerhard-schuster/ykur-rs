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
ykurctrl [-s SERIAL] -u PORT     switch on
ykurctrl [-s SERIAL] -d PORT     switch off
ykurctrl [-s SERIAL] -g PORT     print whether it is on
ykurctrl [-s SERIAL] -v          print the firmware version of the board
ykurctrl -l                      list the attached boards
```

PORT is `1` to `4` for the relay driver ports, `r` for the on-board relay, `a` for all four
ports (the relay is left alone). `-s` picks a board by serial number when several are attached;
without it the first one is used. The switches are those of Yepkit's `ykurcmd`, including
`a` and `-l`, which its manual does not list, with the same meaning; `-g a` is an addition.

Every switching command is confirmed by reading the state back; a target that did not switch is
an error. The exit code is 0 on success and 1 on any error.

Run one invocation at a time per board. Neither macOS nor Linux refuses a second open, and each
answer of the board reaches every open handle, so two invocations at once can read each other's
answers. The echo check turns most of those mix-ups into an error rather than a wrong result,
but two identical commands cannot be told apart.

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
cargo test                                  # 53 tests and a doctest, no board needed
cargo test -- --ignored --test-threads=1    # 6 more, with a board attached
```

The hardware tests read the firmware version, switch every output on and off one by one, the
four ports at once and everything at once, and put the outputs back the way they found them.
Run them only with nothing attached to the outputs that must not switch.

## Coverage

Line coverage with `-C instrument-coverage`, measured on 2026-10-31 on the Raspberry Pi (Linux
on aarch64) at commit 46044bd, whose code this README describes:

| Run | Lines | Missed |
|---|---|---|
| `cargo test` only, no board needed | 83.85 % | 121 of 749 |
| `cargo test` and the hardware run, board `YK22170` attached | 95.46 % | 34 of 749 |

`cli.rs`, `fake.rs` and `sanitize.rs` are at 100 % or one line short in both runs. With the board,
what stays unreached is:

| Place | Why it is not reached |
|---|---|
| `device.rs` - `Error::NoResponse` and discarding stale answers | needs a board that goes quiet for five seconds |
| `device.rs` - a failing read | needs the board to report a read error |
| `device.rs` - a truncated read | needs a board that delivers only part of a report |
| `device.rs` - an open refused for a reason other than an operating system error | needs the transport to fail the open itself |
| `error.rs` - conversion of an output error | needs standard output to fail |
| `main.rs` - `-l` and a command that opens a board, run in the binary | the binary's own tests do not list boards or open one |
| `main.rs` - `unreachable!()` | help, version and listing are handled before a board is opened |
| `ykur.rs` - the error text naming the four ports or all outputs | needs the board to report an error for those targets |
| the rest, two lines | failure arms of assertions in tests |

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

| System | What was run | Result |
|---|---|---|
| macOS on Apple silicon | the hardware tests; `-l`, `-v`, `-g`, `-u` and `-d` on every port, `r` and `a`; `-s` | all pass |
| Raspberry Pi 4 Model B, Debian 13, kernel 6.18 (aarch64) | the same, with the udev rule from [Linux](#linux) | all pass |
| Raspberry Pi, without the udev rule | `-l`, then `-g a` | `-l` lists the board; `-g a` exits 1 and names the rule file |

## Where it differs from ykurcmd

Compared with Yepkit's `ykurcmd` (GitHub, commit ae4a095), from its source:

| | `ykurcmd` | `ykurctrl` |
|---|---|---|
| Answer to a switching command | read and discarded | echo checked, then the state read back; a target that did not switch is an error |
| Exit code | 0, whatever happened | 0 on success, 1 on any error |
| No arguments | prints the help, exit 0 | usage error, exit 1 |
| `-g a` | "Unable to get status" | the state of the four ports |
| `-g` output | `Port 1 is ON` | `Port 1: on` |
| `-v` output | `Firmware Version: Rev.1.2.1` | `Firmware 1.2.1` |
| `-l` output | `Attached YKUR Boards` and a list | `YKUR boards on this host:` and a numbered list |

The switches themselves, `-s`, `-u`, `-d`, `-g`, `-v` and `-l`, and the ports `1` to `4`, `r`
and `a` mean the same in both. Several boards without `-s`: both use the first one found.

## Sources

The vendor documents are not kept here; they belong to Yepkit. See [doc/README.md](doc/README.md).

## License

Apache-2.0, see [LICENSE](LICENSE).
