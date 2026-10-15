# OpenFlow: agent guide

OpenFlow is a free, fully local voice-dictation app in the style of Wispr Flow. You hold a hotkey, speak, and cleaned-up text is pasted into the focused app. Everything runs on-device. There are no cloud code paths, API keys, or accounts. Models download from Hugging Face on first run.

## Hard rules

- **Local only.** No HTTP client may talk to anything except `huggingface.co` (model downloads). No telemetry.
- **License.** MIT. Only MIT, Apache-2.0, BSD, ISC, Zlib or MPL-2.0 dependencies. Never GPL, LGPL or AGPL. Code adapted from Handy (MIT, `vendor/handy`, reference only and not committed) needs an attribution comment naming the source file.
- **Stay inside the repo.** Run cargo with `CARGO_HOME=/workspace/openflow/.cargo-home` so the registry cache stays local. Do not install system packages or touch `~`.
- **No network publishing.** No `git push`, no package publishing.
- **The dev box is headless Linux** with no mic, no display, no C++ compiler and no webkit. macOS and Windows code must compile-gate cleanly. Verify it with `cargo check -p openflow-platform --target aarch64-apple-darwin` and `--target x86_64-pc-windows-gnu`.

## Layout

| Crate | Owns | Depends on |
|---|---|---|
| `crates/core` (`openflow-core`) | Pure logic: hotkey state machine (`hotkey`), text rules (`text`), dictionary, snippets, styles, guardrail, prompts, settings, history, the dictation controller. Defines the traits `SpeechToText`, `VoiceActivity`, `TextGenerator` (`engine.rs`) and `Desktop` (`desktop.rs`). | serde only |
| `crates/engines` | Implements the engine traits: WAV decode and resample, Silero VAD and Parakeet TDT through ONNX Runtime (`ort`, `load-dynamic`), Whisper and quantized LLMs through `candle`, the Hugging Face downloader. Features: `cuda`, `metal`, `coreml`. | core |
| `crates/platform` | Implements `Desktop` and the hotkey listeners per OS: macOS (CGEventTap, AX API through objc2), Windows (low-level keyboard hook, SendInput), Linux X11. Microphone capture with `cpal`. | core |
| `crates/cli` (`openflow` binary) | The headless surface: `transcribe`, `clean`, `dictate --wav`, `eval`, `models`, `daemon`. This is how features are verified on Linux. | all |
| `app/src-tauri` | Tauri 2 shell: tray or menu bar, settings window, onboarding. It only wires the crates above. | all |

The dependency rule is that core depends on nothing heavy. Business logic goes in core as pure functions and is tested there with the mocks in `core::mock`. Engines and platform code stay thin adapters.

## Data flow

```
hotkey listener -> ChordTracker -> KeyEvent -> Machine -> Action
Action::StartRecording -> audio capture (16 kHz mono f32)
Action::Finish(mode)   -> VAD trim -> SpeechToText (hints: dictionary hotwords, language)
  Dictation: rules (fillers, self-corrections, spoken commands, lists, tidy)
             -> dictionary post-replace -> snippet whole-match short-circuit
             -> LLM cleanup (skipped under ~6 words or when disabled) -> guard -> style tone
             -> snippet expansion -> paste with clipboard restore -> history
  Command:   read selection (AX, then copy fallback) -> LLM edit -> replace selection,
             or a notification when no text field is focused
```

## Models (downloaded to the app data dir; `models/` in the repo for dev, gitignored)

- STT default: Parakeet TDT 0.6B v3 int8 ONNX (`istupakov/parakeet-tdt-0.6b-v3-onnx`). Files: `nemo128.onnx` (preprocessor), `encoder-model.int8.onnx`, `decoder_joint-model.int8.onnx`, `vocab.txt`.
- VAD: Silero VAD ONNX (`onnx-community/silero-vad`, `onnx/model.onnx`).
- Multilingual fallback: Whisper large-v3-turbo (`openai/whisper-large-v3-turbo`, safetensors) through candle. Tests use `openai/whisper-tiny`.
- LLM default: Qwen3-4B-Instruct-2507 Q4_K_M GGUF. Fast option: Qwen3-1.7B Q4_K_M (tests use this). Alternatives: Gemma-3-1B-it, Llama-3.2-3B-Instruct.
- ONNX Runtime is loaded at runtime from `ORT_DYLIB_PATH`. On the dev box use `vendor/onnxruntime/onnxruntime-linux-x64-1.31.0/lib/libonnxruntime.so`.

## Commands

```bash
export CARGO_HOME=/workspace/openflow/.cargo-home
export ORT_DYLIB_PATH=/workspace/openflow/vendor/onnxruntime/onnxruntime-linux-x64-1.31.0/lib/libonnxruntime.so
cargo test --workspace                     # unit tests; model tests skip when models/ is absent
cargo test --workspace -- --include-ignored  # also runs the real-model tests
cargo check -p openflow-platform --target aarch64-apple-darwin
cargo check -p openflow-platform --target x86_64-pc-windows-gnu
```

## Conventions

- Rust 2024 edition, `anyhow` at boundaries, typed domain values inside.
- Tests assert literal expected values through the public API. Real-model tests are `#[ignore]` with a reason string and read models from `$OPENFLOW_MODELS` (default `<repo>/models`).
- Comments only for a non-obvious *why*.
