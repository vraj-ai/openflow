# Handoff: Claude Code -> pi (2026-10-15)

Claude Code build (run-claude.sh, claude -p) was stopped at ~18:45 IST while it hung for ~87 min on
`cargo build --release -p openflow-cli` + CLI smoke test. Uncommitted work is committed on branch
`wip/claude-handoff` (merge it into master or continue on it).

## Done (committed on master)
- f44ed1e M1: workspace scaffold, core traits, hotkey state machine
- 5c852af M1: dev env script (`scripts/dev-env.sh`, POSIX-safe), ignore worktrees

## Done but only in wip/claude-handoff (not yet verified as a milestone)
- core: text rules (fillers, self-corrections, lists, spoken punctuation, tidy), dictionary, snippets,
  per-app styles, guardrails, distance; 66 core tests reported passing.
- core: `catalog.json` + `catalog.rs` (11 models pinned to HF commit SHA + SHA-256), `controller.rs`
  (recording -> insert text / replace selection / notification).
- engines: audio, Silero VAD, Parakeet (fixture speech_en.wav: 0% WER, ~1.1 s), ort env, LLM (llama) engine,
  tests `tests/llm.rs`, `tests/parakeet_vad.rs`.
- cli: `openflow models list|transcribe|dictate` and `eval.rs` (ASR WER, cleanup WER, guard rejections, latency).
- Worker sub-branches wip/{corelib,engines,llm,text,whisper} (.worktrees/) were merged by Claude; prompts in .audit/.
  wip/whisper (M6 Whisper fallback) had no commits yet.

## In progress
- M2 LLM cleanup: prefix-cache rewrite of `generate` in crates/engines/src/llm.rs (pristine + cached-prefix KV
  weights). Test passes when run single-threaded; parallel tests OOM (1.1 GB model x3) -> use `--test-threads=1`.
- Downloader not yet wired into engines. PROGRESS.md does not exist yet.
- Last action: release build of openflow-cli then `dictate ... --app com.tinyspeck.slackmacgap --no-llm`; result unknown.

## Next steps
1. `. scripts/dev-env.sh`; build + `cargo test` (single-threaded for engines); fix failures; commit M1 remainder / M2.
2. Create PROGRESS.md. Finish M2 (eval set, toggles, history), then M3..M6 per BRIEF.md.
3. Mac-only parts (CGEventTap, AX, DMG) must compile-gate; record what can't be verified on this Linux box.
