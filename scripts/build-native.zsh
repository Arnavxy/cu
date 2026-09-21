#!/bin/zsh
set -eu

ROOT="${0:A:h:h}"
OUT="${1:-$ROOT/bin/cu-native}"

mkdir -p "${OUT:h}"
xcrun swiftc \
  -O \
  -framework AppKit \
  -framework CoreGraphics \
  -framework Vision \
  "$ROOT/Sources/cu-native/main.swift" \
  -o "$OUT"

codesign --force --sign - "$OUT" >/dev/null 2>&1 || true
print -r -- "built $OUT"
