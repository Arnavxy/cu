#!/bin/zsh
# Four standalone Rust executables. Never packages voice, private state, or Python.
set -eu
ROOT="${0:A:h:h}"
TARGET="${1:-$(rustc -vV | awk '/^host:/ {print $2}')}"
OUT="${2:-$ROOT/dist}"
case "$TARGET" in
  aarch64-apple-darwin) ARCH=arm64 ;;
  x86_64-apple-darwin) ARCH=x86_64 ;;
  *) print -u2 "unsupported release target: $TARGET"; exit 2 ;;
esac
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/cu-runtime/Cargo.toml" | head -1)"
[[ "$VERSION" == 0.5.0 ]] || { print -u2 "unexpected release version: $VERSION"; exit 2; }
LIBDIR="$(rustc --print target-libdir --target "$TARGET")"
[[ -d "$LIBDIR" ]] || { print -u2 "Missing Rust standard library for $TARGET. Install with rustup target add $TARGET, or use the macOS release workflow."; exit 2; }
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --locked --release --manifest-path "$ROOT/cu-runtime/Cargo.toml" --target-dir "$ROOT/cu-runtime/target" --target "$TARGET" --bins
mkdir -p "$OUT"
OUT="${OUT:A}"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/cu-release.XXXXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
NAME="cu-$VERSION-$TARGET"
mkdir -p "$STAGE/$NAME/bin" "$STAGE/$NAME/scripts"
for BIN in cu cu-native cu-input cu-mcp; do
  SOURCE="$ROOT/cu-runtime/target/$TARGET/release/$BIN"
  [[ "$(lipo -archs "$SOURCE")" == "$ARCH" ]] || { print -u2 "wrong architecture: $BIN"; exit 1; }
  install -m 755 "$SOURCE" "$STAGE/$NAME/bin/$BIN"
  codesign --force --sign - "$STAGE/$NAME/bin/$BIN" >/dev/null 2>&1
  codesign --verify --strict "$STAGE/$NAME/bin/$BIN"
done
install -m 755 "$ROOT/scripts/install.zsh" "$STAGE/$NAME/scripts/install.zsh"
install -m 644 "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/CHANGELOG.md" "$ROOT/SECURITY.md" "$STAGE/$NAME/"
cp -R "$ROOT/docs" "$STAGE/$NAME/docs"
COPYFILE_DISABLE=1 tar -czf "$OUT/$NAME.tar.gz" -C "$STAGE" "$NAME"
(cd "$OUT" && shasum -a 256 "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
print -r -- "$OUT/$NAME.tar.gz"
