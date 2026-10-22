#!/bin/zsh
# Release/install smoke test uses a disposable destination and never ~/bin.
set -eu
ROOT="${0:A:h:h}"
ARCHIVES=("$ROOT"/dist/*.tar.gz(N))
ARCHIVE="${1:-${ARCHIVES[1]:-}}"
[[ -f "$ARCHIVE" ]] || { print -u2 'build a release archive first'; exit 1; }
TMP="$(mktemp -d "${TMPDIR:-/tmp}/cu-package-test.XXXXXXXX")"
trap 'rm -rf "$TMP"' EXIT
(cd "${ARCHIVE:h}" && shasum -a 256 -c "${ARCHIVE:t}.sha256")
tar -xzf "$ARCHIVE" -C "$TMP"
PACKAGES=("$TMP"/cu-*(/))
[[ ${#PACKAGES} == 1 ]] || exit 1
PACKAGE="$PACKAGES[1]"
[[ ! -e "$PACKAGE/cu-runtime" && ! -e "$PACKAGE/voice-runtime" && ! -e "$PACKAGE/Sources" ]]
CU_INSTALL_DIR="$TMP/installed" "$PACKAGE/scripts/install.zsh"
[[ "$("$TMP/installed/cu" --version)" == 'cu 0.5.0' ]]
RESPONSE="$(print -r -- '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | "$TMP/installed/cu-mcp")"
[[ "$RESPONSE" == *'0.5.0'* ]] || { print -u2 'MCP version mismatch'; exit 1; }
for BIN in cu cu-native cu-input cu-mcp; do
  [[ -x "$TMP/installed/$BIN" ]]
  codesign --verify --strict "$TMP/installed/$BIN"
done
print -r -- 'PASS: checksums, isolated binary installation, CLI and MCP versions, four signatures'
