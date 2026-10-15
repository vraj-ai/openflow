# OpenFlow — build brief

## Goal
Build a free, open-source, fully local clone of Wispr Flow (AI voice dictation into any app), following the plan below end to end (all 6 milestones). Working name in the plan is "LocalFlow"; repo name is openflow.

## Hard constraints
- Fully local at runtime: no cloud code paths, no paid APIs. Models download from Hugging Face on first run.
- No purchases, no signups, no logins.
- Do NOT push to any remote, publish packages/releases, open PRs/issues, or post anything anywhere. Local git only. (The user must say yes first.)
- License: MIT (add LICENSE). Forking/vendoring MIT code is fine with attribution: evaluate forking Handy (github.com/cjpais/Handy, verified MIT, Tauri+Rust, has Parakeet v3 + Whisper + Silero VAD) as the base rather than starting from scratch. Do not pull in GPL/AGPL code.
- Stay inside /workspace/openflow. Do not modify global config or other directories (cloning into /workspace/openflow/vendor or similar is fine).
- This box is Linux without a mic/display: macOS-specific code (CGEventTap, AX API) must compile-gate cleanly; verify on Linux with `cargo build`/`cargo test`, unit tests with fixture WAVs, and mocks for audio/hotkey/paste. Be honest in PROGRESS.md about what could not be verified here.

## How to work
- Use the pstack skills (plugin `pstack`): poteto-mode for rigor; architect for design; tdd for implementation; prove-it-works before claiming any milestone done; review skills before commits.
- You are the orchestrator/architect. Delegate implementation chunks to pi subagents, non-interactively, e.g.:
  `pi -p --no-session --model opencode-go/deepseek-v4.1-flash --thinking high "<precise task with file paths, acceptance tests>"`
  Run them from /workspace/openflow. Give each a narrow, testable task; review their diffs; re-run tests yourself. Run independent pi tasks in parallel where safe. If pi is failing/hanging, note it in PROGRESS.md and do the work yourself.
- Verify each milestone with builds and tests, then commit per milestone (`git commit -m "M<n>: ..."`).
- Keep PROGRESS.md up to date: milestone status, what was verified and how, open issues, next steps.

## The plan (verbatim)
# Build plan: free, open-source, fully local Wispr Flow clone ("working name: LocalFlow")
_Date: 2026-10-15. This is a plan only. Nothing has been built. All effort, latency and RAM numbers marked (est.) are estimates._

## 1. What Wispr Flow actually does (from public sources)
- **Push-to-talk into any app**: hold a hotkey (default Fn on Mac, Ctrl+Win on Windows), speak, release, and formatted text is pasted into whatever field is focused. A tap under 0.5 s is ignored. Esc cancels. **Hands-free** mode starts with Fn+Space or a double-tap and stops on the next press. There is an optional experimental "press enter" voice command. Sources: [first dictation](https://docs.wisprflow.ai/articles/6409258247-starting-your-first-dictation), [hands-free](https://docs.wisprflow.ai/articles/6391241694-use-flow-hands-free), [shortcuts](https://docs.wisprflow.ai/articles/2612050838-supported-unsupported-keyboard-hotkey-shortcuts)
- **AI edits**: removes filler words, auto-punctuates, formats numbered lists, and handles spoken self-corrections ("no wait, Tuesday"). Uses surrounding context to spell names. Works when whispering. Supports 100+ languages. Source: [features](https://wisprflow.ai/features)
- **Personal dictionary**: learns automatically when you correct a word, and you can add words by hand. **Snippets** expand a spoken cue into saved text. **Styles** set tone per app (formal in docs, casual in chat), English and desktop only. Team plans share the dictionary and snippets. Source: [features](https://wisprflow.ai/features)
- **Command Mode** (experimental): hold Fn+Ctrl and speak an instruction ("make this more concise", "draft a reply"). It edits the selected or surrounding text, or answers in a notification if no text field is focused. Source: [Command Mode](https://docs.wisprflow.ai/articles/4816967992-how-to-use-command-mode)
- **Developer features**: file tagging, syntax awareness, dev jargon. **Platforms**: Mac, Windows, iPhone (keyboard), Android (floating bubble). Source: [what is Flow](https://docs.wisprflow.ai/articles/2772472373-what-is-flow)
- Flow is cloud-based and paid. That gap is the reason to build ours.

## 2. Existing open-source alternatives, and how ours differs
| Project | Stack / license | Local STT | LLM cleanup | Per-app styles | Command mode | Gap |
|---|---|---|---|---|---|---|
| [Handy](https://github.com/cjpais/Handy) | Tauri+Rust, MIT, Mac/Win/Linux | Whisper, Parakeet v3, Silero VAD | None per [OpenWhispr's comparison](https://openwhispr.com/compare/handy) (verify current state) | No | No | Transcription only |
| [OpenWhispr](https://github.com/openwhispr/openwhispr) | Electron, MIT | Whisper, Parakeet (sherpa-onnx), llama.cpp | Yes ("agent", prompts); cloud-first defaults | Partial | Agent | Heavy Electron app, cloud and Pro upsell, sprawling scope (meetings, notes) |
| [VoiceInk](https://github.com/Beingpax/VoiceInk) | Swift, GPL-3, macOS 15+ only | whisper.cpp, Parakeet (FluidAudio) | Optional (often a cloud LLM) | Power Mode per app/URL | Assistant | Mac-only, paid binaries |
| whisper-writer, Whispering | Python / Svelte+Tauri (AGPL) | Whisper | Minimal | No | No | Rough user experience |
| Superwhisper | Closed source, paid | – | – | – | – | Not open source |

Sources: [Reveneau comparison](https://reveneau.com/open-source/alternatives-to/wispr-flow), [OpenWhispr vs VoiceInk](https://openwhispr.com/compare/voiceink).

**Our niche, so we don't duplicate existing work:** a lightweight Tauri app that is local-only by design, with no cloud code paths, and that matches Flow's *editing* layer: a local LLM for cleanup and self-corrections, per-app styles, a dictionary that learns from your corrections, snippets, and Command Mode on selected text. No meetings, notes, or accounts.

**Strong alternative to building from scratch (recommended to evaluate first):** fork Handy (MIT, same stack) or contribute these features upstream as a post-processing pipeline. That could cut M1 almost entirely.

## 3. Scope
**MVP (macOS on Apple Silicon first):**
- Push-to-talk and hands-free modes
- VAD and Parakeet speech-to-text
- Rule-based filler removal plus LLM cleanup, which can be turned off
- Insertion via clipboard paste with the previous clipboard restored
- Personal dictionary (used as a speech-model hint and an LLM glossary) and snippets
- Per-app style chosen by the frontmost app's bundle ID
- History, model downloader, menu-bar UI

**Later:**
- Command Mode (read selection via Accessibility, LLM edit, replace)
- Auto-learning the dictionary from your corrections (watch the text field for edits)
- Windows/Linux with CUDA (your 3070/3090 boxes)
- Streaming partial text, "press enter" voice command, URL-level styles for browsers, multilingual Whisper fallback
- Mobile: out of scope

## 4. Stack
- **Recommended: Tauri 2 (Rust core with a small React or Svelte UI).** It is cross-platform for your Mac and both PCs, has a small footprint, and has proven prior art (Handy). Hotkeys come from a CGEventTap on macOS (needed for Fn and hold detection; the plain global-shortcut plugin can't hold Fn), and rdev or Windows hooks elsewhere. Audio via `cpal`. Paste via `enigo` plus the clipboard. Selection read and frontmost app via the macOS AX API, wrapped in an `objc2` shim.
- **Swift-native** gives the best Apple Neural Engine (ANE) access (FluidAudio, MLX-Swift) and Accessibility integration, but it is Mac-only and duplicates VoiceInk. Rejected for the core. If ANE Parakeet matters, a small Swift sidecar or FluidAudio via FFI is an option.
- **Inference**: Rust bindings to `whisper-rs` (whisper.cpp), `sherpa-onnx` or `transcribe-rs` for Parakeet (CoreML on Mac, CUDA on the PCs), and `llama-cpp-2` (llama.cpp, with Metal and CUDA) for the LLM. Everything runs in-process, with models downloaded from Hugging Face on first run.

## 5. Local speech model (M4)
| Model | English WER | Speed on Apple Silicon | Notes |
|---|---|---|---|
| **Parakeet TDT 0.6B v3** | 2.50% (50 clips, MacBook Air M4 16GB); 6.32% avg on Open ASR leaderboard | ~0.10 s per utterance on a MacBook Air M4; ~100–200× realtime on M4 Pro | 25 European languages, punctuation, almost no hallucination on silence. Sources: [plainsay bench](https://github.com/conrader/plainsay/blob/main/BENCHMARK.md), [FluidAudio benchmarks](https://github.com/FluidInference/FluidAudio/blob/main/Documentation/Benchmarks.md), [Whisper Notes](https://whispernotes.app/blog/parakeet-v3-default-mac-model) |
| Whisper large-v3-turbo | 2.89% (same M4 Air test); 7.83% on the leaderboard | ~0.68 s per utterance on M4 Air; ~14× realtime on M4 Pro | 100+ languages including Hindi; prompt biasing supported. Same sources |
| Moonshine (tiny/base) | Worse than both (est.) | Very fast, tiny | English-only. A good choice for streaming previews or weak CPUs, not as the main model |

**Pick:** Parakeet v3 as the default (v2 for English-only, slightly better English WER). Whisper large-v3-turbo as the fallback for Hindi and other languages. On the 3090/3070 both are effectively instant (est.).

## 6. Local LLM for cleanup (16 GB budget)
- Memory budget: Parakeet ~0.5–1 GB, plus LLM at Q4 ~2.5–3 GB, plus app = under 5 GB (est.), leaving room for normal Mac use.
- **Default: Qwen3-4B-Instruct (Q4_K_M GGUF) via llama.cpp with Metal.** Fallbacks are Gemma-3-4B-it and Llama-3.2-3B-Instruct. There is a "fast" option, Qwen3-1.7B or Gemma-3-1B, for a latency of ~0.3 s or less. Check for newer 2026 small models at build time.
- Expected latency for cleaning a ~50-word utterance on M4: about 0.5–1.5 s with a 4B Q4 model (est.: prompt prefill plus ~60–80 tokens out at roughly 30–40 tok/s). Keep the prompt short, cache it with llama.cpp's KV prefix, constrain output, and skip the LLM entirely for utterances under ~6 words (rules only).
- MLX (`mlx-lm`) is somewhat faster on Mac but Python- and Mac-only. Keep llama.cpp for one codebase.
- Prompt design: fixed system prompt (no fillers, apply self-corrections, format lists, never answer questions or add content), plus dictionary glossary, plus the style for the current app. Fall back to the raw transcript if the output drifts too far from it (edit-distance guard).

## 7. Milestones (effort is an estimate for one dev plus pi agent sessions)
1. **M1 – Core dictation loop (1–1.5 wk, est.)**: Tauri shell and menu bar, Fn/hotkey push-to-talk via event tap, cpal audio, Silero VAD, Parakeet speech-to-text, paste with clipboard restore, permissions onboarding (Microphone, Accessibility). *Exit:* under 500 ms from key release to text in any app, on the M4.
2. **M2 – Cleanup pipeline (1 wk, est.)**: rule-based filler removal, llama.cpp Qwen3-4B cleanup, self-correction handling, list formatting, guardrails, toggles, history view. Eval set of ~100 recorded utterances with WER and edit metrics.
3. **M3 – Personalisation (1 wk, est.)**: dictionary (speech-model hotwords/prompt plus LLM glossary plus post-replace), snippets, per-app styles by bundle ID, settings UI, model manager and downloader.
4. **M4 – Command Mode (1–1.5 wk, est.)**: second hotkey, read selection or nearby text through AX (clipboard fallback), LLM edit, replace. If no text field is focused, show the answer in a notification. Add hands-free mode and Esc cancel.
5. **M5 – Windows/Linux and CUDA (1–1.5 wk, est.)**: Windows hooks and paste, CUDA builds for whisper and llama, tested on 3070/3090, larger default LLM option on the PCs (e.g. 8–14B).
6. **M6 – Polish and release (1 wk, est.)**: auto-learn the dictionary from corrections, Whisper multilingual fallback, signed/notarized DMG (an Apple developer ID costs money; otherwise ship ad-hoc signed with instructions), CI, docs.
Total: ~6–8 weeks part-time-equivalent (est.).

## 8. Coding with pi
- Verified with `~/.local/bin/pi --list-models`: **`opencode-go/deepseek-v4.1-flash`** exists (1M context, 384K output). Run e.g. `pi --model opencode-go/deepseek-v4.1-flash`.
- Work one milestone per session, write an AGENTS.md describing the architecture and the crates above, and run pi against Handy's source for reference patterns (hotkey handling, AX, paste) while respecting the MIT license.

## 9. Risks
- macOS permissions and Fn-key capture are fiddly.
- Paste fails in some apps (terminals, Electron apps).
- The LLM may "answer" the dictated text instead of cleaning it, so guardrails and evals are needed.
- Without notarization, Gatekeeper will warn users.
- Overlap with Handy: decide fork vs. new repo before M1.
