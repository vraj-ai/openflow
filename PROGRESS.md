# PROGRESS — OpenFlow build

Branch: `wip/claude-handoff` (continues the Claude Code handoff; nothing is pushed anywhere).
Last updated: 2026-10-15, session 2 (pi builder).

## How this is verified (and what cannot be)

This box is headless Linux: 8 cores, 16 GB RAM, **no microphone, no display, no GPU, no
C++ toolchain, no webkit**. Verification therefore happens through the headless CLI against
real on-device models, plus `cargo check` for the macOS/Windows targets.

| Verified here | Not verifiable here |
|---|---|
| `cargo build` / `cargo test` for the whole workspace | Real hotkey capture (CGEventTap, low-level Windows hook) |
| Real Parakeet int8 + Silero VAD + Whisper + GGUF LLM inference | Real microphone capture quality and paste into a live app |
| The whole dictation pipeline via `openflow dictate <wav>` against a mocked desktop | The `<500 ms` release-to-paste exit criterion (needs an M4 Mac) |
| ASR WER / latency on 73 recorded LibriSpeech clips | Tauri UI (the Linux build needs webkit2gtk; it is not installed and must not be) |
| Cleanup WER / latency on a 45-case cleanup eval set | Code signing / notarisation (needs an Apple developer ID) |
| macOS + Windows crates compile (`cargo check --target …`) | Actual macOS/Windows runtime behaviour |

Models present in `models/` (gitignored): `parakeet-tdt-0.6b-v3`, `silero-vad`, `whisper-tiny`,
`qwen3-1.7b` (GGUF). The default cleanup model (`qwen3-4b-instruct-2507`) and
`whisper-large-v3-turbo` are **not** downloaded on this box, so cleanup numbers below come from
Qwen3-1.7B Q4_K_M (the documented "fast" option) and multilingual STT is exercised with
`whisper-tiny`.

## Milestones

| # | Milestone | Status |
|---|---|---|
| M1 | Core dictation loop | **Done** (verified headlessly; macOS/Windows runtime untested) |
| M2 | Cleanup pipeline + eval set | **Done** |
| M3 | Personalisation (dictionary, snippets, styles, model manager, settings) | **Done** (CLI settings surface; no GUI) |
| M4 | Command Mode, hands-free, Esc cancel | **Done** (verified headlessly) |
| M5 | Windows/Linux + CUDA | **Partly done** — compile-gated; CUDA needs the GPU boxes |
| M6 | Polish and release (auto-learn, Whisper fallback, CI, docs) | Not started |

---

## M1 — Core dictation loop

Implemented (commits `f44ed1e`, `5c852af`, `f8316f5`, `a602849`, `9efa8b1`, `6d53b16`):

- `crates/core`: hotkey state machine (hold/release, tap discard under `min_hold_ms`, double-tap
  hands-free, `Fn+Space` hands-free chord, Esc cancel), traits `SpeechToText`, `VoiceActivity`,
  `TextGenerator`, `Desktop`, the `Controller` (record → insert text), `Runtime` loop, settings,
  history, mocks.
- `crates/engines`: WAV decode/resample, Silero VAD (ONNX Runtime, loaded via `ORT_DYLIB_PATH`),
  Parakeet TDT 0.6B v3 int8 (ONNX), Whisper via candle, GGUF LLM via candle, Hugging Face
  downloader with resume + SHA-256, model catalog pinned to HF commit SHAs.
- `crates/platform`: `Desktop` implementations per OS (macOS CGEventTap/AX via objc2, Windows
  low-level hook + SendInput, Linux X11), microphone capture with `cpal`, permission probing.
- `crates/cli`: `transcribe`, `clean`, `dictate --wav`, `models list|download`, `eval`, `daemon`.

Evidence (all green in this session):

```
. scripts/dev-env.sh
cargo test --workspace                 # core 135, platform 4, engines integration 1, cli 0 failures
cargo test -p openflow-engines -- --include-ignored --test-threads=1
                                       # 7 passed: 3 LLM (incl. prefix-cache), 4 Parakeet/VAD (real models)
cargo check -p openflow-platform --target aarch64-apple-darwin
cargo check -p openflow-platform --target x86_64-pc-windows-gnu
./target/debug/openflow transcribe crates/engines/tests/fixtures/speech_en.wav
  -> "mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel." (3.1 s cold)
./target/debug/openflow eval --asr eval/librispeech.jsonl
  -> ASR 73 clips, 481 s audio: WER 4.17% | latency p50 424 ms, p95 728 ms | 14.3x realtime (CPU)
```

Open issues for M1: the Tauri shell and menu bar in the plan do not exist — the crates are laid
out so an `app/` crate can drive the same controller, but the UI itself cannot be compiled on this
box (no webkit2gtk, and system packages must not be installed). Latency numbers are CPU-only.

## M2 — Cleanup pipeline

Implemented/verified this session (on top of the merged `wip/*` branches):

- Rule passes in `crates/core/src/text`: fillers, self-corrections, spoken commands
  (new line/paragraph, "press enter"), list formatting, tidy (spacing, punctuation, sentence
  case, `i` → `I`, and **new**: apostrophe restoration for `dont/lets/im/ive/youre/...`).
- Rule fixes found by the eval set:
  - fillers now also disappear *between* two words ("we should uh ship it"), while unit-like
    tokens ("5 mm") stay guarded;
  - self-corrections anchor on the replaced value's class ("three" → "four pm", "Monday" →
    "Tuesday"), so `lets meet at three no wait four pm on tuesday` becomes
    "Let's meet at four pm on tuesday" instead of dropping the whole head; a long restart with no
    anchor is left for the LLM instead of being cut blindly;
  - spoken "new line" no longer gets re-formatted into a numbered list, and the list rule fires on
    unpunctated spoken runs ("first the report second the slides") while keeping prose like
    "a second opinion";
  - accepted LLM output is now run through `tidy`, which removed the trailing-space artefacts the
    model adds before newlines (exact matches on list cases went 1/22 → 5/22).
- History view: `openflow history [--limit N] [--clear] [--json]` plus `history::format_time`.
- Toggles: `settings.cleanup.{llm_enabled, min_words_for_llm, rules, guard}`, insertion,
  history, learning (all persisted JSON, defaults for missing fields).
- Eval sets: `eval/librispeech.jsonl` (73 recorded clips + `eval/audio/`) and **new**
  `eval/cleanup.jsonl` (45 hand-written cases tagged filler/correction/list/spoken/tidy/question/
  style/dev/long/hard).

Evidence:

```
./target/debug/openflow eval --cleanup eval/cleanup.jsonl --no-llm
  -> 45 cases: WER 1.93% (raw transcript 28.43%) | exact 7/45 | p50 0 ms
./target/debug/openflow --settings .scratch/settings-1.7b.json eval --cleanup eval/cleanup.jsonl
  -> 45 cases with Qwen3-1.7B Q4 on CPU: WER 1.93% | exact 4/45 (pre-tidy-fix run)
     per-case: LLM better 0, worse 0, same 32; latency p50 6.6 s, p95 52 s
```

Reading of those numbers: the deterministic rules already carry the word-level quality
(1.93% WER vs 28.4% raw); the LLM's contribution in this measurement is punctuation and question
marks (exactness), not WER. On CPU the 1.7B model is too slow for interactive use (the first call
after a style change pays a ~50 s system-prompt prefill; generation runs ~2–3 tok/s). The plan's
0.5–1.5 s budget assumes Metal/CUDA, which is M5's job; the guard, prompt and prefix cache are in
place and measured.

Known weak cases (kept in the eval set on purpose): "turn the volume up no wait down a bit"
(antonym correction below the LLM word threshold), a long self-correction restart, and tone
rewriting for the formal mail style — a 1.7B model does not do them; the 4B default is expected to
do better but is not downloaded here.

## M3 — Personalisation

Implemented and verified this session:

- **Settings surface without a UI** (the headless box has no display): `openflow set <key> <value>`
  with a whitelist of settable keys, `openflow dict list|add|remove [--sounds-like …]`,
  `openflow snippet list|add|remove`, `openflow models list|download|remove`, and
  `openflow history [--limit] [--clear] [--json]`. `set` validates model ids against the catalog
  ("whisper-tiny is a Multilingual model, not Llm") and yells at unknown keys with the key list.
- **Dictionary** — STT hotwords, LLM glossary and a post-replace pass with sounds-like phrases;
  **snippets** — whole-utterance cue expansion plus inline cue expansion with an LLM-survival
  check; **per-app styles** — bundle id / app name / URL host matchers, 4 tones.
- **Model manager** — catalog pinned to HF commit SHAs, resumable download with SHA-256 check,
  `--yes`-gated removal.

Evidence:

```
# dictionary + snippet through the real Parakeet model and the insert path
./target/debug/openflow --settings .scratch/m3-settings.json \
    dictate crates/engines/tests/fixtures/speech_en.wav --no-llm --app com.apple.mail
  raw: "mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel."
  -> pasted "Mister Quilter is the OpenFlow, and we are glad to welcome the OpenFlow launch."
     (dictionary sounds-like "apostle of the middle classes" -> OpenFlow; snippet "his gospel")

# model manager against a scratch models dir (real network, hash-checked)
OPENFLOW_MODELS=.scratch/models openflow models download silero-vad   # 2 MB installed
OPENFLOW_MODELS=.scratch/models openflow models remove silero-vad     # would delete … pass --yes
OPENFLOW_MODELS=.scratch/models openflow models remove silero-vad --yes

# history view (core test covers the write path: history_records_inserted_text)
OPENFLOW_HOME=.scratch/home openflow history
  2025-10-15 16:20:00 dictation com.apple.mail           Send the deck by Friday.
  2025-10-15 17:20:00 command   com.tinyspeck.slackmacgap Shorter.  [llm rejected]
```

Tone evidence: the deterministic half of a style is observable end-to-end — the same utterance
through `openflow clean --app com.apple.mail` (formal) keeps/closes the final period while
`--app com.tinyspeck.slackmacgap` (casual) drops it. The *wording* did not change between the two
tones with Qwen3-1.7B, so per-app register currently rests on the prompt, which a 1.7B model
largely ignores; re-measure with the 4B default on a Metal/CUDA box.

Not verified: the plan's "settings UI" as a GUI — the Tauri shell does not exist here; the CLI
commands above are the settings surface, and the same `Settings`/`Dictionary`/`Snippets` types are
what a UI would bind to.

## M4 — Command Mode, hands-free, Esc cancel

Implemented in the tree and verified this session (the code came with the merged `wip/*`
branches; nothing needed fixing):

- Second hotkey (`Fn+Ctrl` / `Ctrl+Win`) selects Command Mode; `Controller::command` reads the
  selection through the AX API and falls back to a sentinel-copy trick that restores the previous
  clipboard, then replaces the selection with the LLM's rewrite. With no selection it uses the
  focused field as context and inserts; with no text field at all it puts the answer on the
  clipboard and raises a notification. `prompt::command_prompt` has the three shapes.
- Hands-free: a double tap or the `Fn+Space` chord starts recording that ends on the next press.
- Escape cancels while recording and is swallowed so it does not reach the app underneath.

Evidence:

```
# selection replaced through the real LLM and paste path
openflow --settings .scratch/m3-settings.json dictate crates/engines/tests/fixtures/speech_en.wav \
    --mode command --selection "the quarterly migration plan needs a shorter summary ..."
  -> outcome Replaced("the quarterly migration plan needs a shorter summary ..."), keys [Paste]

# no text field anywhere -> notification + clipboard, nothing pasted
openflow --settings .scratch/m3-settings.json dictate crates/engines/tests/fixtures/speech_en.wav --mode command
  -> outcome Notified(...), keys [Copy], notifications [["OpenFlow", ...]], pasted []

cargo test -p openflow-core hotkey::   # double_tap_enters_hands_free_and_next_press_finishes,
                                       # hands_free_chord_starts_hands_free, escape_cancels_and_swallows_release,
                                       # escape_cancels_hands_free, escape_when_idle_does_nothing
cargo test -p openflow-core desktop::  # selection_falls_back_to_copy_and_restores_clipboard
```

The two CLI runs used the LibriSpeech fixture as the spoken instruction, so they prove the plumbing
(transcript -> selection read -> LLM -> replace/notify), not instruction following; instruction
quality is covered by the command-prompt unit tests and the M2 note about model size. The macOS AX
read itself cannot be exercised here.

## M5 — Windows, Linux, CUDA

What the tree already has (verified by compile-gating; there is no Windows or GPU machine here):

- `crates/platform/src/windows.rs`: `WH_KEYBOARD_LL` hook, `SendInput` paste/Enter, foreground
  executable name as the app id, clipboard through `arboard`; selection uses the copy fallback (no
  UI Automation reader yet). Linux X11 hotkey listener and paste live in `linux.rs`; macOS
  compile-gates as CGEventTap + AX.
- `crates/engines` features `cuda`, `metal`, `coreml`; CUDA selects `Device::new_cuda(0)`, Metal
  selects `Device::new_metal(0)`.
- The catalog carries the bigger PC-side models (`qwen3-8b`, `qwen3-14b`) next to the Mac default
  (`qwen3-4b-instruct-2507`), and `openflow set cleanup.llm_model qwen3-8b` switches to one.

Evidence:

```
cargo check -p openflow-platform --target x86_64-pc-windows-gnu   # ok
cargo check -p openflow-platform --target aarch64-apple-darwin    # ok
cargo check -p openflow-engines --features cuda                   # FAILS here: the cudarc build
  # script runs `nvcc --version` and this box has no CUDA toolkit. Needs a 3070/3090 machine.

openflow doctor      # new: settings + models dir, per-model installed state, what the settings
                     # point at, ONNX Runtime path, cuda/metal/coreml built+available, permissions
  cuda:       built false, available false
  metal:      built false, available false
  onnxruntime: .../libonnxruntime.so (true)
  settings point at a model that is not downloaded: whisper-large-v3-turbo
                     # on a 3070/3090 box: build with --features cuda, then `openflow doctor`
                     # should print `built true, available true` before switching llm_model
```

Not verified: any runtime behaviour on Windows/Linux desktops, Wayland (X11 only), and CUDA/Metal
inference and their speedups. This is the milestone where this box cannot produce evidence.
