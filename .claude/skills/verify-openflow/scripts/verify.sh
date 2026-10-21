#!/usr/bin/env bash
# Builds OpenFlow, runs the workspace tests, and drives the release `openflow`
# CLI the way a user would. Every command, its stdout, stderr and exit code
# land in .verify/<sha>-<utc-ts>-<random>/. Exits 1 when any step FAILs and 2
# when VERIFY_LOG_DIR already holds files.
set -uo pipefail

SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(git -C "$SKILL_DIR" rev-parse --show-toplevel)"
# Overrides are relative to where the caller ran this, not to the repo root
# the script moves into next.
abs() { realpath -ms -- "$1"; }
CALLER_MODELS=""
[ -n "${OPENFLOW_MODELS:-}" ] && CALLER_MODELS="$(abs "$OPENFLOW_MODELS")"
CALLER_LOG=""
[ -n "${VERIFY_LOG_DIR:-}" ] && CALLER_LOG="$(abs "$VERIFY_LOG_DIR")"
cd "$ROOT"
# shellcheck source=/dev/null
. scripts/dev-env.sh
[ -n "$CALLER_MODELS" ] && export OPENFLOW_MODELS="$CALLER_MODELS"
# A rustup install with toolchains but no default makes every cargo call fail;
# pin stable for this process instead of writing a default into ~/.rustup.
if command -v rustup > /dev/null && ! rustup show active-toolchain > /dev/null 2>&1; then
  export RUSTUP_TOOLCHAIN=stable
fi

SHA="$(git rev-parse HEAD)"
DIRTY="$(git status --porcelain --untracked-files=no | grep -q . && echo yes || echo no)"
if [ -n "$CALLER_LOG" ]; then
  # Reusing a dir would mix this run's files with an older run's evidence.
  if [ -e "$CALLER_LOG" ] && { [ ! -d "$CALLER_LOG" ] || [ -n "$(ls -A -- "$CALLER_LOG")" ]; }; then
    echo "verify.sh: VERIFY_LOG_DIR=$CALLER_LOG exists and is not an empty directory" >&2
    exit 2
  fi
  mkdir -p "$CALLER_LOG"
  LOG="$CALLER_LOG"
else
  # mktemp -d creates the dir atomically, so concurrent runs in the same
  # second still get separate evidence dirs and settings homes.
  mkdir -p "$ROOT/.verify"
  LOG="$(mktemp -d "$ROOT/.verify/${SHA:0:12}-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")"
fi
# The CLI keeps settings and history under OPENFLOW_HOME; a per-run home keeps
# the user's real settings untouched and leaves the run's settings as evidence.
export OPENFLOW_HOME="$LOG/home"
mkdir -p "$OPENFLOW_HOME"

BIN="$ROOT/target/release/openflow"
TRANSCRIPT="$(cat "$SKILL_DIR/fixtures/filler-transcript.txt")"
WAV="crates/engines/tests/fixtures/speech_en.wav"
SPEECH_RAW="mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel."
SPEECH_PASTED="Mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel"

N=0
OUT=""
RC=0
FAILED=0
printf 'step\tstatus\texit\tsecs\tnote\n' > "$LOG/steps.tsv"

# run <step> <cmd...>: runs the command, saving NN-step.{cmd,stdout,stderr,exit}.
run() {
  local step=$1; shift
  N=$((N + 1))
  local base t0
  base="$(printf '%s/%02d-%s' "$LOG" "$N" "$step")"
  printf '%q ' "$@" > "$base.cmd"; echo >> "$base.cmd"
  printf '\n$ %s' "$(cat "$base.cmd")" >&2
  t0=$SECONDS
  "$@" > "$base.stdout" 2> "$base.stderr"
  RC=$?
  SECS=$((SECONDS - t0))
  echo "$RC" > "$base.exit"
  OUT="$base.stdout"
  printf '  -> exit %d (%ds)\n' "$RC" "$SECS" >&2
}

# record <step> <PASS|FAIL|SKIPPED> <note>: appends to steps.tsv. SKIPPED rows
# have no command, so their exit and secs columns are "-".
record() {
  local exit_col="-" secs_col="-"
  [ "$2" != SKIPPED ] && exit_col=$RC && secs_col=$SECS
  [ "$2" = FAIL ] && FAILED=1
  printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$exit_col" "$secs_col" "$3" >> "$LOG/steps.tsv"
}

# expect <step> <want-stdout> <note>: PASS iff exit 0 and stdout is exactly want.
expect() {
  local got
  got="$(cat "$OUT")"
  if [ "$RC" -eq 0 ] && [ "$got" = "$2" ]; then
    record "$1" PASS "$3"
  else
    record "$1" FAIL "exit $RC, stdout '$got', want '$2'"
  fi
}

model_ok() { grep -Eq "^[[:space:]]+$1[[:space:]]+ok$" "$LOG/doctor.stdout" 2>/dev/null; }

run build cargo build --release --workspace
BUILT=1
if [ "$RC" -eq 0 ] && [ -x "$BIN" ]; then BUILT=0; record build PASS "$BIN"; else record build FAIL "cargo build exit $RC"; fi

run test cargo test --workspace
TOTALS="$(awk '/^test result:/ {p+=$4; f+=$6; i+=$8} END {printf "%d passed, %d failed, %d ignored", p, f, i}' "$OUT")"
if [ "$RC" -eq 0 ]; then record test PASS "$TOTALS"; else record test FAIL "exit $RC; $TOTALS"; fi

if [ "$BUILT" -ne 0 ]; then
  for s in doctor clean-rules clean-llm transcribe dictate; do record "$s" SKIPPED "release build failed"; done
else
  run doctor "$BIN" doctor
  cp "$OUT" "$LOG/doctor.stdout"
  if [ "$RC" -eq 0 ] \
    && grep -Fqx "models dir: $OPENFLOW_MODELS" "$OUT" \
    && grep -Fqx "onnxruntime: $ORT_DYLIB_PATH (loads)" "$OUT"; then
    record doctor PASS "models dir resolved and onnxruntime loads"
  else
    record doctor FAIL "exit $RC, models dir line missing, or onnxruntime does not load"
  fi

  run clean-rules "$BIN" clean --no-llm "$TRANSCRIPT"
  expect clean-rules "So I think we should ship it on on Friday" "rule pass removed um/uh"

  if model_ok qwen3-1.7b; then
    run set-llm "$BIN" set cleanup.llm_model qwen3-1.7b
    if [ "$RC" -ne 0 ]; then
      record clean-llm FAIL "openflow set cleanup.llm_model exit $RC"
    else
      run clean-llm "$BIN" clean "$TRANSCRIPT"
      expect clean-llm "So I think we should ship it on Friday" "Qwen3-1.7B removed fillers and the stutter"
    fi
  else
    record clean-llm SKIPPED "doctor: qwen3-1.7b not downloaded in $OPENFLOW_MODELS"
  fi

  if model_ok parakeet-tdt-0.6b-v3 && model_ok silero-vad; then
    run transcribe "$BIN" transcribe "$WAV"
    expect transcribe "$SPEECH_RAW" "Parakeet + Silero on $WAV"

    run dictate "$BIN" dictate --no-llm "$WAV"
    if [ "$RC" -eq 0 ] && grep -Fqx '  "keys": "[Paste]",' "$OUT" \
      && grep -Fq "\"pasted\": [" "$OUT" && grep -Fqx "    \"$SPEECH_PASTED\"" "$OUT"; then
      record dictate PASS "simulated desktop received only the paste chord and the cleaned text"
    else
      record dictate FAIL "exit $RC, keys not exactly [Paste], or pasted text missing"
    fi
  else
    record transcribe SKIPPED "doctor: parakeet-tdt-0.6b-v3 or silero-vad not downloaded in $OPENFLOW_MODELS"
    record dictate SKIPPED "doctor: parakeet-tdt-0.6b-v3 or silero-vad not downloaded in $OPENFLOW_MODELS"
  fi
fi

{
  echo
  echo "OpenFlow verify  HEAD $SHA  (dirty tracked files: $DIRTY)"
  echo "evidence: $LOG"
  echo
  awk -F'\t' '{printf "%-12s %-8s %-5s %-5s %s\n", $1, $2, $3, $4, $5}' "$LOG/steps.tsv"
  echo
  if [ "$FAILED" -eq 0 ]; then echo "RESULT: PASS (no FAIL rows)"; else echo "RESULT: FAIL"; fi
} | tee "$LOG/summary.txt"

exit "$FAILED"
