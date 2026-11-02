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
  then gives up
- interference between programs that have the device open at once, which
  Linux does not prevent; every answer goes to all of them. On macOS this
  program opens the device exclusively. See below for what it does about it
  and what it cannot do

## Concurrent use

On Linux, invocations of `ykurctrl` take turns at a board through an
advisory `flock` on its `/dev/hidrawN`. Advisory means it only orders the
programs that ask for it: the instances of `ykurctrl` and programs built on
its library. It is not access control and does not try to be one.

Nothing protects a run of Yepkit's `ykurcmd`, or of any other HID tool,
against the same board in parallel. Such a tool opens the device beside
this program, and answers can be read by the wrong process. Any local user
the udev rule lets open the node can also take the lock and hold it, which
makes this program wait and then fail; that is the denial of service above,
with a limit.

The rule that follows: **one YKUR tool per host**.

On macOS this program opens the device exclusively (seized,
`Access::Exclusive` from sidit77/async-hid#55). While it holds the device,
every other open through the HID stack fails, from this program or any
other, and a second invocation waits for the first. A program that already
had the device open keeps its handle, but its writes fail while the seize
lasts. A tool that bypasses the HID stack is not covered. In turn, another
program that seizes the device makes this one wait and then fail, the
denial of service above. Measured on 2026-11-02 on a Mac: two invocations
reading at the same time failed in none of 5080 runs, against about half
before the exclusive open.

## What has already been looked at

No security review of this program has been recorded yet.
