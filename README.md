# OpenFlow

Free, open-source, fully local dictation. Hold a hotkey, speak, release: cleaned-up text is pasted
into whatever app has focus. Everything — speech recognition, voice-activity detection, the
cleanup model — runs on your machine. No account, no API key, no telemetry, no request to any host
except `huggingface.co` when you download a model.

MIT licensed. Working name in the build plan was "LocalFlow"; the repo is `openflow`.

## What it does

- **Push-to-talk** — hold the dictate key, release to paste. A tap under `min_hold_ms` is ignored,
  Esc cancels.
- **Hands-free** — double-tap the dictate key, or press the hands-free chord (default `Fn+Space`);
  the next press stops.
- **Cleanup** — filler removal, self-corrections ("no wait, Tuesday"), list formatting, spoken
  commands ("new line", "press enter"), sentence case and punctuation, then an optional local LLM
  pass with a drift guard. Utterances under `min_words_for_llm` (default 6) skip the LLM.
- **Command Mode** — a second hotkey edits the selection ("make this more concise") through the
  cleanup model; if no text field is focused, the answer goes to a notification.
- **Personalisation** — dictionary (speech-model hotwords + LLM glossary + post-replace), snippets,
  and per-app tones matched by bundle id, app name or URL host.
- **Learns from you** — after a paste, it watches the field briefly and adds the words you corrected
  to the dictionary (`learning.auto_learn`).
- **Multilingual fallback** — Parakeet TDT 0.6B v3 covers 25 European languages; setting
  `stt.language` to anything else routes to Whisper.

## Install

There is no signed installer yet. Build from source (Rust 2024 edition, Rust 1.85+):

```bash
git clone <this repo> && cd openflow
sudo apt-get install -y libasound2-dev     # Linux only: cpal links ALSA
cargo build --release -p openflow-cli      # target/release/openflow
```

Models are not vendored. Download the ones the settings point at:

```bash
export OPENFLOW_MODELS=$PWD/models        # optional; defaults to the app data dir
./target/release/openflow models download parakeet-tdt-0.6b-v3
./target/release/openflow models download silero-vad
./target/release/openflow models download qwen3-4b-instruct-2507   # default cleanup model
./target/release/openflow doctor          # what is installed, which accelerator is live
```

ONNX Runtime is loaded at runtime from `ORT_DYLIB_PATH` (grab the CPU build for your platform from
the ONNX Runtime releases page). On macOS and Windows `cargo build` picks the native backends up;
`--features cuda` on an NVIDIA box selects CUDA for Whisper and the LLM, `--features metal` on
Apple Silicon selects Metal.

## Use it

```bash
openflow daemon                # hold the hotkey and dictate; the real thing
openflow doctor                # environment, models, permissions
openflow models list           # catalog: speech, VAD, multilingual, cleanup models
openflow history --limit 20    # what was dictated
openflow settings              # the settings file and its contents
```

Headless surfaces (also how this repo is verified on a box without a microphone):

```bash
openflow transcribe clip.wav                         # speech-to-text only
openflow clean "so um we should uh ship it friday"   # cleanup rules only
openflow dictate clip.wav --app com.tinyspeck.slackmacgap   # full pipeline against a simulated desktop
openflow eval --asr eval/librispeech.jsonl           # ASR WER and latency
openflow eval --cleanup eval/cleanup.jsonl --no-llm  # cleanup quality, rules only
```

Settings, dictionary and snippets are plain JSON under the app data dir (`openflow settings` prints
the path); `openflow set <key> <value>` writes one field at a time:
`stt.model`, `stt.multilingual_model`, `stt.language`, `stt.vad`,
`cleanup.llm_enabled`, `cleanup.llm_model`, `cleanup.min_words_for_llm`, `cleanup.rules.*`,
`insertion.*`, `history.*`, `learning.*`, `styles.default_tone`.
Hotkeys are set in the same file (`hotkeys.dictate`, `hotkeys.command`, `hotkeys.hands_free`).

## Layout

| Crate | Owns |
|---|---|
| `crates/core` | Pure logic: hotkey state machine, text rules, dictionary, snippets, styles, guardrail, prompts, settings, history, the dictation controller. Traits for speech-to-text, VAD, text generation and the desktop. |
| `crates/engines` | ONNX Runtime (Silero VAD, Parakeet) and candle (Whisper, GGUF LLMs), WAV decode/resample, Hugging Face downloader with SHA-256 verification. |
| `crates/platform` | Hotkey listeners and `Desktop` per OS: macOS CGEventTap + Accessibility API, Windows low-level hook + SendInput, Linux X11; microphone capture with cpal. |
| `crates/cli` | The `openflow` binary: `transcribe`, `clean`, `dictate`, `daemon`, `eval`, `models`, settings, dictionary, snippets, history, `doctor`. |

Data flow, model choices and the reasoning behind them are in `AGENTS.md`; the build history and
what could be verified on which machine is in `PROGRESS.md`.

## Privacy

- No analytics, no crash reporting, no update check, no accounts. The only HTTP client in the tree
  talks to `huggingface.co` for model downloads (`crates/engines/src/download.rs`).
- Audio is held in memory for the length of one utterance and never written to disk.
- History is a local JSONL file you can clear (`openflow history --clear`) or disable
  (`openflow set history.enabled off`).

## Attribution

- Whisper inference follows the reference implementation through
  [candle](https://github.com/huggingface/candle)'s MIT/Apache-2.0 `examples/whisper`.
- Model weights come from the Hugging Face repos pinned in `crates/core/src/catalog.json`; each
  entry records the commit SHA and file sizes (and SHA-256 where published).
- The design approaches [Handy](https://github.com/cjpais/Handy) (MIT) solves were studied as
  reference for hotkey capture, paste and VAD around a Tauri app; no Handy code is vendored here.
- Dependencies are MIT, Apache-2.0, BSD, ISC, Zlib or MPL-2.0 only. No GPL/LGPL/AGPL.

## Status

Six build milestones are complete and verified as far as a headless Linux box allows: builds and
tests run on Linux, macOS and Windows compile-gates, and the dictation pipeline was exercised
end-to-end against real models through the CLI. Not verified anywhere: real hotkey capture and
paste into live apps, microphone quality, the GPU backends, and code signing/notarisation. Details,
numbers and open issues: `PROGRESS.md`. Release packaging: `docs/RELEASING.md`.
