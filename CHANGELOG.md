# Changelog

Notable changes, newest first. Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

First version. `ykurctrl` switches the relay and the four ports of a YKUR, reads their state
and the firmware version, and lists the attached boards, on macOS on Apple silicon and Linux and
FreeBSD on aarch64, all three run against a board. Switching is confirmed by reading the state
back, since firmware 1.2.1 answers switching commands differently from the datasheet.

The exit status follows sysexits(3) and says what to do next: `64` invalid command line, `69`
no board, board unreachable or the board reported an error, `74` no answer or transport error,
`75` board in use, `76` an answer that does not fit the request, `77` access denied; the same
codes as `ykushctrl`.

Two invocations at once take turns at a board: on Linux through an advisory `flock` on the
hidraw node, on FreeBSD through the open itself, since hidraw allows one at a time, on macOS
through an exclusive open (`Access::Exclusive` from sidit77/async-hid#55,
pinned on the fork until it is released; measured on a Mac with a board). The second one
waits up to 5 seconds, or fails at once with `--no-wait`. The Linux side has not yet run on the
Raspberry Pi with that pinned revision; its hidraw path is unchanged for the default open.

After the security review in `SECURITY-REVIEW-0.1.0.md`: before its first command, and after an
exchange that failed or got an answer that does not fit, the program asks for the firmware
version, drops every report before its answer and waits 50 ms for the queue to stay quiet, so
an answer left behind by a killed invocation is not read as the answer to the next one. On macOS writes give up after 5 seconds like reads.
Serial numbers and arguments are printed with everything outside printable ASCII replaced.

A waiting invocation keeps waiting while the board comes back from a reset or a power cycle
while another program held it, on macOS and FreeBSD, up to the end of the wait; on FreeBSD,
while the board is held, only its own node is opened again, so other programs do not fail to
open their own devices with `EBUSY` meanwhile. Without `-s`, FreeBSD takes the first board by
the path of its device node even while it is held, and `-l` lists the boards in that order. A
board unplugged after it was opened ends with `69` where the transport reports it as
disconnected; on Linux the pinned revision reports it as `ENODEV`, which ends with `74`.
Measured with a YKUR on a Mac: a board held while its power was switched off and on was got by 4
of 4 waiting invocations after about 2.1 s. Measured on FreeBSD in a VM: no other program failed
to open its device in 3000 opens while an invocation waited, against 23 failures with the
earlier listing on every attempt; the wait across a re-enumeration was measured there with a
reset YKUSH3 only, with a 15 s wait, since the board takes about 5 s to come back in the VM.
