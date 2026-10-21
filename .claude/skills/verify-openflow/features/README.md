# OpenFlow verification map

This map lists the user-facing features an agent can drive on the headless dev box through the release `openflow` CLI. `scripts/verify.sh` drives one path through each of the four: the `doctor` checks, `clean-rules` and `clean-llm`, `transcribe-parakeet`, and `dictate-paste`. It does not drive `clean-style`, `transcribe-no-vad`, `transcribe-whisper`, `dictate-command` or `dictate-learn`. Use a feature file as the recipe when checking one of those by hand.

## Baseline preconditions

- Build with `cargo build --release --workspace` after sourcing `scripts/dev-env.sh`.
- Export `OPENFLOW_HOME` to a fresh directory so settings and history start from defaults and the user's real ones stay untouched.
- Run `target/release/openflow doctor` and read the model table. A feature that needs a `missing` model is SKIPPED, not verified.

## Proof and skip reporting

- CLI proof is the command, stdout, stderr and exit code.
- Cleaned text goes to stdout. Timing and `llm:` / `rejected:` diagnostics go to stderr.
- Do not report a skipped feature as verified through another one.

## Features

- [Doctor](./doctor.md) covers the machine health report.
- [Text cleanup](./clean.md) covers the rule pass and the LLM pass on typed text.
- [Transcribe](./transcribe.md) covers speech-to-text on a WAV.
- [Dictate](./dictate.md) covers the full pipeline into a simulated desktop, including the Command Mode and learn sub-features, which verify.sh does not run.
