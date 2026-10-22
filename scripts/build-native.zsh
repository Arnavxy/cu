#!/bin/zsh
set -eu
ROOT="${0:A:h:h}"
OUT="${1:-$ROOT/bin/cu-native}"
cargo build --locked --release --manifest-path "$ROOT/cu-runtime/Cargo.toml" --target-dir "$ROOT/cu-runtime/target" --bin cu-native
mkdir -p "${OUT:h}"
install -m 755 "$ROOT/cu-runtime/target/release/cu-native" "$OUT"
codesign --force --sign - "$OUT" >/dev/null 2>&1
print -r -- "built $OUT"
