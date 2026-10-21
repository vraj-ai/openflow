---
name: verify-openflow
description: Build OpenFlow, run the workspace tests, and drive the release `openflow` CLI (doctor, clean, transcribe, dictate) on the headless Linux dev box, capturing every command, output and exit code as evidence. Use after changing any crate, before claiming a feature works, or when asked to verify OpenFlow end to end.
---

# Verify OpenFlow

OpenFlow's user surfaces are the global-hotkey daemon, the Tauri tray app, and the `openflow` CLI. The dev box has no microphone, display, or webkit, so the CLI is the surface an agent can drive. `openflow dictate --wav` runs the full pipeline against a simulated desktop and prints what would be pasted. That makes it the closest stand-in for the live hotkey path.

## Run it

```bash
.claude/skills/verify-openflow/scripts/verify.sh
```

It exits 0 when no step FAILs and 1 otherwise. A cold release build plus the test build takes several minutes. Warm reruns take about 40 s, most of it the Qwen3-1.7B cleanup pass.

Environment knobs:

- `OPENFLOW_MODELS=<dir>` points at a different models dir. The default is `<repo>/models`. Point it at an empty dir to check that the model steps report SKIPPED.
- `VERIFY_LOG_DIR=<dir>` replaces the default evidence dir. The script exits 2 before running anything if that path already exists and is not an empty directory, so one run's evidence never mixes with another's.

Relative paths in either knob resolve against the directory you ran the script from.

## Launch

There is no server. "Launch" means `cargo build --release --workspace` (step `build`), producing `target/release/openflow`. The script sources `scripts/dev-env.sh`, which sets `CARGO_HOME`, `ORT_DYLIB_PATH` and `OPENFLOW_MODELS` per AGENTS.md. When rustup has toolchains but no default, the script exports `RUSTUP_TOOLCHAIN=stable` for its own process rather than writing to `~/.rustup`.

Each run sets `OPENFLOW_HOME=<evidence dir>/home`, so the CLI reads and writes a fresh `settings.json` there and never touches the user's real settings or history. Two runs can go side by side, even in the same second, because the default evidence dir ends in a random suffix and each run gets its own home.

## Doctor

`openflow doctor` is the read-only health check (step `doctor`). The step requires exit 0, a `models dir:` line equal to `$OPENFLOW_MODELS`, and an `onnxruntime: <ORT_DYLIB_PATH> (loads)` line. The CLI prints `(loads)` only when it initializes ONNX Runtime from that library, and `(failed: <reason>)` otherwise, so a corrupt library fails this step even when no model is installed. The model table it prints decides which later steps run. A model listed as `missing` turns its steps into SKIPPED with the reason, never PASS.

## Drive

The steps run in this order. Each asserts a literal value.

| Step | Command | Pass condition |
|---|---|---|
| `build` | `cargo build --release --workspace` | exit 0 and the binary exists |
| `test` | `cargo test --workspace` | exit 0. The note sums the passed, failed and ignored counts |
| `doctor` | `openflow doctor` | see Doctor |
| `clean-rules` | `openflow clean --no-llm "<fixture>"` | stdout is `So I think we should ship it on on Friday` |
| `clean-llm` | `openflow set cleanup.llm_model qwen3-1.7b`, then `openflow clean "<fixture>"` | stdout is `So I think we should ship it on Friday`. Needs `qwen3-1.7b` |
| `transcribe` | `openflow transcribe crates/engines/tests/fixtures/speech_en.wav` | stdout is the LibriSpeech reference sentence. Needs `parakeet-tdt-0.6b-v3` and `silero-vad` |
| `dictate` | `openflow dictate --no-llm <same wav>` | `"keys": "[Paste]"` exactly, so an extra key such as Enter fails, and the `pasted` array holds the cleaned sentence. Same models |

The fixture is `fixtures/filler-transcript.txt` (`um, so, uh, I think we should ship it on on Friday`). The WAV is the committed LibriSpeech clip (CC BY 4.0, 183 KB) that the engine tests already use.

To drive a single feature by hand, see `features/README.md`.

## Evidence

Everything lands in `.verify/<sha12>-<utc-ts>-<random>/`, which is gitignored:

- `NN-<step>.cmd`, `.stdout`, `.stderr` and `.exit` for every command that ran.
- `steps.tsv` with one row per step: step, status, exit, secs, note.
- `summary.txt`, the same table printed at the end, headed by the full HEAD SHA and whether tracked files were dirty.
- `home/settings.json`, the settings the run used.

A PASS means the real release binary produced the asserted output from real models. Report the `summary.txt` table and the evidence path, not a paraphrase. A SKIPPED step is not verified. Say which steps were skipped and why.

## Cleanup

The script starts no long-lived processes and leaves the evidence dir in place. To reclaim space, delete old `.verify/*` dirs by hand. Never delete the dir you are citing.

## Not covered

The daemon's hotkey listener, microphone capture, and real paste/AX calls need desktop hardware and cannot run here. `dictate` covers the same controller with a simulated desktop. macOS and Windows code is only compile-checked: run `cargo check -p openflow-platform --target aarch64-apple-darwin` and `--target x86_64-pc-windows-gnu` when touching `crates/platform`.
