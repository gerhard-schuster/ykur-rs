# Changelog

Notable changes, newest first. Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

First version. `ykurctrl` switches the relay and the four ports of a YKUR, reads their state
and the firmware version, and lists the attached boards, on macOS on Apple silicon and Linux on
aarch64, both run against a board. Switching is confirmed by reading the state back, since
firmware 1.2.1 answers switching commands differently from the datasheet.

The exit status follows sysexits(3) and says what to do next: `64` invalid command line, `69`
no board, board unreachable or the board reported an error, `74` no answer or transport error,
`75` board in use, `76` an answer that does not fit the request, `77` access denied; the same
codes as `ykushctrl`.
