# Reporting a vulnerability

Report privately to the maintainer, Gerhard Schuster
<gerhard.schuster@44qm.net>, rather than opening a public issue. That address is
the one in the commit history, so it is not new information.

Say what the program does wrong and how to bring it about. A patch is welcome but
not expected. Expect an answer within a week; if none arrives, the report has gone
astray and is worth sending again.

Only the current state of the default branch is maintained. There are no
supported older releases to backport a fix to.

## What is in scope

This program talks to one USB device and prints what it says. The interesting
boundary is what comes back from that device: it is not trusted, and anything
that makes the program mishandle a hostile or broken board is in scope. So is
anything reachable through the command line.

Out of scope, because they are properties of the hardware rather than of this
program:

- that any local process allowed to open the device can switch the relay and
  the ports - macOS hands HID devices to any process that asks, on Linux the
  udev rule decides who may open the hidraw node, and this program neither
  adds nor could add a check there
- what the relay and the ports switch: the board has no notion of what is
  attached, and neither has this program
- firmware behaviour of the board itself, which belongs to Yepkit
- denial of service by holding the device open; an invocation of this
  program waits at most five seconds for the board, described below, and
  then gives up. Once it has the board, every read gives up after five
  seconds, and so does every write on macOS; on Linux the write runs
  inside the kernel, whose USB timeout bounds it rather than this program.
  An invocation makes several exchanges, the version query included, so a
  board that stops answering holds it for a bounded time, not for five
  seconds in all
- interference between programs that have the device open at once, which
  Linux does not prevent; every answer goes to all of them. On macOS this
  program opens the device exclusively. See below for what it does about it
  and what it cannot do

## Concurrent use

On Linux, invocations of `ykurctrl` take turns at a board through an
advisory `flock` on its `/dev/hidrawN`. Advisory means it only orders the
programs that ask for it: the instances of `ykurctrl` and programs built on
its library. It is not access control and does not try to be one.

On Linux, nothing protects a run of Yepkit's `ykurcmd`, or of any other HID
tool, against the same board in parallel. Such a tool opens the device beside
this program, and answers can be read by the wrong process. Any local user
the udev rule lets open the node can also take the lock and hold it, which
makes this program wait and then fail; that is the denial of service above,
with a limit.

The rule that follows: **one YKUR tool per host**.

On FreeBSD hidraw lets one program open a node at a time, so the open is
the turn and holds back every program that opens the node; a program that
keeps it open makes this one wait and then fail, as above. While waiting,
the board's own node is opened every 10 ms once it is known; the other
nodes are opened only to find it.

On macOS this program opens the device exclusively (seized,
`Access::Exclusive` from sidit77/async-hid#55). While it holds the device,
every other open through the HID stack fails, from this program or any
other, and a second invocation waits for the first. A program that already
had the device open keeps its handle, but its writes fail while the seize
lasts; that was measured in sidit77/async-hid#55 with a YKUSH3, not with a
YKUR. A tool that bypasses the HID stack is not covered. In turn, another
program that seizes the device makes this one wait and then fail, the
denial of service above. Measured on 2026-11-02 on a Mac: two invocations
reading at the same time failed in none of 5080 runs, against about half
before the exclusive open.

## Known limitations

- Boards are told apart by serial number. Two boards with the same serial
  number, or both without one, are listed as one, and `-s` picks whichever
  the system enumerates first. A board that copies the serial number of
  another one therefore takes its place. Telling them apart by their
  position on the bus is possible but not done.
- The protocol has no request ids. The program gets back in step by
  asking for the firmware version, dropping every report before its
  answer and waiting 50 ms for the queue to stay quiet after it, since a
  version answer may itself have been left behind. An answer that takes
  longer than that to follow is read by the next command; every command
  checks its echo, so it is reported (exit `76`) rather than taken for a
  wrong value, unless that command is itself a version query.

## What has already been looked at

[SECURITY-REVIEW-0.1.0.md](SECURITY-REVIEW-0.1.0.md) records the review of
the trust boundary to the device, the command line, the locking and the
dependency surface, and what was changed afterwards. It was taken without
the author's working context, but not by an outside party; it is a record
of what was examined, not an independent audit.
