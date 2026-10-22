# Changelog

All notable changes to OpenFlow are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

- Added the `verify-openflow` skill. `.claude/skills/verify-openflow/scripts/verify.sh` builds the workspace, runs the tests and drives the release CLI, saving each command's output and exit code as evidence.
- `openflow doctor` now reports whether ONNX Runtime loads, printing `(loads)` or `(failed: <reason>)` instead of only checking that the path exists.
- README: added the demo GIF and a link to the full MP4.
- README: the Wispr Flow comparison now cites its cloud-only and offline claims to Wispr Flow's help center, and the "Account required" row is gone.
