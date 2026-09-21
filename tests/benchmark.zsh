#!/bin/zsh
# Lightweight, deterministic latency smoke test for the semantic fast path.
# It uses the same mocked backends as tests/test_cu.zsh, so it never touches
# the desktop. Timing is intentionally informational: real UI latency belongs
# to the target application and cannot be inferred from a mock.
set -eu

ROOT="${0:A:h:h}"
CU="$ROOT/bin/cu"
TMP="${TMPDIR:-/tmp}/cu-benchmark.$$.${RANDOM}"
mkdir -p "$TMP"
trap 'rm -rf "$TMP"' EXIT

zmodload zsh/datetime
start=$EPOCHREALTIME
observation=$( \
  CU_DIR="$TMP/state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  "$CU" observe Demo --json
)
snapshot=$(print -r -- "$observation" | sed -n 's/.*"snapshot":"\([^"]*\)".*/\1/p')
element=$(print -r -- "$observation" | sed -n 's/.*"id":"\(e_[0-9]*\)".*/\1/p' | head -n 1)

CU_DIR="$TMP/state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  CU_VERIFY_DELAY=0 \
  "$CU" act "$element" --snapshot "$snapshot" --json >/dev/null
elapsed=$(( EPOCHREALTIME - start ))

print -r -- "semantic observe + verified action (mock backends): ${elapsed}s"
print -r -- "Real app latency is measured separately; this guards cu's own command overhead."
