#!/bin/zsh
set -eu

ROOT="${0:A:h:h}"
DEST="${CU_INSTALL_DIR:-$HOME/bin}"

"$ROOT/scripts/build-native.zsh"
mkdir -p "$DEST"
install -m 755 "$ROOT/bin/cu" "$DEST/cu"
install -m 755 "$ROOT/bin/cu-native" "$DEST/cu-native"
print -r -- "installed cu and cu-native to $DEST"
