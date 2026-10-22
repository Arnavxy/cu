#!/bin/zsh
# Works in a source checkout or an extracted release; no Python runtime needed.
set -eu
ROOT="${0:A:h:h}"
DEST="${CU_INSTALL_DIR:-$HOME/bin}"
if [[ -f "$ROOT/cu-runtime/Cargo.toml" ]]; then
  cargo build --locked --release --manifest-path "$ROOT/cu-runtime/Cargo.toml" --target-dir "$ROOT/cu-runtime/target" --bins
  SOURCE="$ROOT/cu-runtime/target/release"
else
  SOURCE="$ROOT/bin"
fi
for NAME in cu cu-native cu-input cu-mcp; do
  [[ -f "$SOURCE/$NAME" && -x "$SOURCE/$NAME" ]] || { print -u2 "missing executable: $SOURCE/$NAME"; exit 1; }
done
# Refuse a wrong-architecture/nonworking CLI and directory-shaped destinations
# before replacing any existing executable. Version inspection does no GUI work.
"$SOURCE/cu" --version >/dev/null
mkdir -p "$DEST"
for NAME in cu cu-native cu-input cu-mcp; do
  [[ ! -d "$DEST/$NAME" ]] || { print -u2 "refusing to replace a directory: $DEST/$NAME"; exit 1; }
done
STAGE="$(mktemp -d "$DEST/.cu-install.XXXXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
for NAME in cu cu-native cu-input cu-mcp; do
  install -m 755 "$SOURCE/$NAME" "$STAGE/$NAME"
done
for NAME in cu cu-native cu-input cu-mcp; do
  mv -f "$STAGE/$NAME" "$DEST/$NAME"
done
print -r -- "installed Rust cu, cu-native, cu-input and cu-mcp to $DEST"
