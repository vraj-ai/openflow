#!/usr/bin/env bash
# Builds OpenFlow, runs the workspace tests, drives the release CLI, and saves
# each command with its stdout, stderr and exit code under .verify/.
# Exit 1: a step FAILed or evidence could not be written. Exit 2: setup failed.
set -uo pipefail

die() { echo "verify.sh: $*" >&2; exit 2; }

SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || die "cannot resolve the skill directory"
ROOT="$(git -C "$SKILL_DIR" rev-parse --show-toplevel)" || die "$SKILL_DIR is not inside a git checkout"
# Overrides are relative to where the caller ran this, not to the repo root
# the script moves into next.
abs() { realpath -ms -- "$1"; }
CALLER_MODELS=""
[ -n "${OPENFLOW_MODELS:-}" ] && CALLER_MODELS="$(abs "$OPENFLOW_MODELS")"
CALLER_LOG=""
[ -n "${VERIFY_LOG_DIR:-}" ] && CALLER_LOG="$(abs "$VERIFY_LOG_DIR")"
cd "$ROOT" || die "cannot cd to $ROOT"
# shellcheck source=/dev/null
. scripts/dev-env.sh || die "scripts/dev-env.sh failed"
[ -n "$CALLER_MODELS" ] && export OPENFLOW_MODELS="$CALLER_MODELS"
for v in CARGO_HOME ORT_DYLIB_PATH OPENFLOW_MODELS; do
  [ -n "${!v:-}" ] || die "scripts/dev-env.sh left $v empty"
done
# Cargo falls back to ~/.cargo when this is relative or missing; AGENTS.md forbids touching ~.
case "$CARGO_HOME" in /*) ;; *) die "CARGO_HOME must be absolute, got $CARGO_HOME" ;; esac
# BIN below is $ROOT/target/release/openflow, so cargo must build exactly there.
export CARGO_TARGET_DIR="$ROOT/target"
unset CARGO_BUILD_TARGET CARGO_BUILD_TARGET_DIR
# The engines honour OPENFLOW_MODEL_DIR_<ID> (crates/engines/src/stack.rs) but
# doctor reads only OPENFLOW_MODELS, so an inherited override would let a step
# pass against a model that doctor reported as missing.
unset "${!OPENFLOW_MODEL_DIR_@}"
# A rustup install with toolchains but no default makes every cargo call fail;
# pin stable for this process instead of writing a default into ~/.rustup.
if command -v rustup > /dev/null && ! rustup show active-toolchain > /dev/null 2>&1; then
  export RUSTUP_TOOLCHAIN=stable
fi

SHA="$(git rev-parse HEAD)" || die "cannot read HEAD"
STATUS="$(git status --porcelain)" || die "git status failed"
if [ -n "$STATUS" ]; then DIRTY=yes; else DIRTY=no; fi
if [ -n "$CALLER_LOG" ]; then
  mkdir -p -- "$(dirname -- "$CALLER_LOG")" || die "cannot create the parent of VERIFY_LOG_DIR=$CALLER_LOG"
  # Plain mkdir is atomic: of two runs given the same path, only one wins.
  mkdir -- "$CALLER_LOG" 2> /dev/null || die "VERIFY_LOG_DIR=$CALLER_LOG already exists or cannot be created; pass a path that does not exist"
  LOG="$CALLER_LOG"
else
  # mktemp keeps concurrent runs in the same second apart.
  mkdir -p "$ROOT/.verify" || die "cannot create $ROOT/.verify"
  LOG="$(mktemp -d "$ROOT/.verify/${SHA:0:12}-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")" || die "cannot create an evidence dir"
fi
# A per-run home keeps the user's real settings and history untouched.
export OPENFLOW_HOME="$LOG/home"
mkdir -p "$OPENFLOW_HOME" || die "cannot create $OPENFLOW_HOME"

BIN="$ROOT/target/release/openflow"
TRANSCRIPT="$(cat "$SKILL_DIR/fixtures/filler-transcript.txt")" || die "cannot read the filler transcript fixture"
WAV="crates/engines/tests/fixtures/speech_en.wav"
SPEECH_RAW="mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel."
SPEECH_PASTED="Mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel"
# Timings go to stderr, so stdout is deterministic and one literal comparison
# also rejects a misplaced sentence, an extra paste or an extra key.
DICTATE_WANT="$(printf '{\n  "keys": "[Paste]",\n  "learned": [],\n  "notifications": [],\n  "outcome": "Inserted(\\"%s\\")",\n  "pasted": [\n    "%s"\n  ]\n}' "$SPEECH_PASTED" "$SPEECH_PASTED")"

N=0
OUT=""
RC=0
FAILED=0
printf 'step\tstatus\texit\tsecs\tnote\n' > "$LOG/steps.tsv" || die "cannot write $LOG/steps.tsv"

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

# A note must not add rows or columns to steps.tsv.
flat() {
  local s
  s="$(printf '%s' "$1" | tr '\t\r\n' '   ')"
  printf '%s' "${s:0:200}"
}

# SKIPPED rows have no command, so exit and secs are "-".
record() {
  local exit_col="-" secs_col="-"
  [ "$2" != SKIPPED ] && exit_col=$RC && secs_col=$SECS
  [ "$2" = FAIL ] && FAILED=1
  printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$exit_col" "$secs_col" "$(flat "$3")" >> "$LOG/steps.tsv" || FAILED=1
}

expect() {
  local got
  got="$(cat "$OUT")"
  if [ "$RC" -eq 0 ] && [ "$got" = "$2" ]; then
    record "$1" PASS "$3"
  else
    record "$1" FAIL "exit $RC, stdout '$got', want '$2'"
  fi
}

model_ok() { awk -v id="$1" '$1 == id && $2 == "ok" { found = 1 } END { exit !found }' "$LOG/doctor.stdout" 2> /dev/null; }

run build cargo build --release --workspace --locked
BUILT=1
if [ "$RC" -eq 0 ] && [ -x "$BIN" ]; then BUILT=0; record build PASS "$BIN"; else record build FAIL "cargo build exit $RC"; fi

run test cargo test --workspace --locked
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
    expect dictate "$DICTATE_WANT" "simulated desktop received only the paste chord and the cleaned text"
  else
    record transcribe SKIPPED "doctor: parakeet-tdt-0.6b-v3 or silero-vad not downloaded in $OPENFLOW_MODELS"
    record dictate SKIPPED "doctor: parakeet-tdt-0.6b-v3 or silero-vad not downloaded in $OPENFLOW_MODELS"
  fi
fi

TABLE="$(awk -F'\t' '{printf "%-12s %-8s %-5s %-5s %s\n", $1, $2, $3, $4, $5}' "$LOG/steps.tsv")" || {
  FAILED=1
  TABLE="(could not read $LOG/steps.tsv)"
}
result_line() { if [ "$FAILED" -eq 0 ]; then echo "RESULT: PASS (no FAIL rows)"; else echo "RESULT: FAIL"; fi; }
BODY="$(printf '\nOpenFlow verify  HEAD %s  (uncommitted or untracked files: %s)\nevidence: %s\n\n%s\n' "$SHA" "$DIRTY" "$LOG" "$TABLE")"
if ! printf '%s\n\n%s\n' "$BODY" "$(result_line)" > "$LOG/summary.txt" || [ ! -s "$LOG/summary.txt" ]; then
  FAILED=1
  echo "verify.sh: could not write $LOG/summary.txt" >&2
fi
printf '%s\n\n%s\n' "$BODY" "$(result_line)"

exit "$FAILED"
