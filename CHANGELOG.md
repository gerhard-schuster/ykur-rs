# Changelog

Notable changes, newest first. Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

First version. `ykurctrl` switches the relay and the four ports of a YKUR, reads their state and
the firmware version, and lists the attached boards, on macOS on Apple silicon and Linux on
aarch64. Switching is confirmed by reading the state back, since firmware 1.2.1 answers
switching commands differently from the datasheet.
