#!/bin/zsh
set -eu

ROOT="${0:A:h:h}"
CU="$ROOT/bin/cu"
TMP="${TMPDIR:-/tmp}/cu-test.$$.${RANDOM}"
mkdir -p "$TMP"
trap 'rm -rf "$TMP"' EXIT

chmod +x "$ROOT/tests/fixtures/mock-osascript"

pass() { print -r -- "ok - $1"; }
fail() { print -r -- "not ok - $1" >&2; exit 1; }
assert_contains() { [[ "$1" == *"$2"* ]] || fail "$3"; }

zsh -n "$CU" || fail "script parses"
pass "script parses"

help="$("$CU" --help)"
assert_contains "$help" "--json" "help documents JSON mode"
pass "help documents JSON mode"

tree=$(
  CU_DIR="$TMP/state" \
  CU_OSASCRIPT="$ROOT/tests/fixtures/mock-osascript" \
  CU_MOCK_OSASCRIPT_RESULT=$'AXButton | Save | 10,20\nAXTextField | Search | 30,40' \
  "$CU" --json tree Demo --all
)
assert_contains "$tree" '"role":"AXButton"' "tree JSON includes roles"
assert_contains "$tree" '"name":"Search"' "tree JSON includes names"
pass "tree JSON"

clicked=$(
  CU_DIR="$TMP/state" \
  CU_OSASCRIPT="$ROOT/tests/fixtures/mock-osascript" \
  CU_MOCK_OSASCRIPT_RESULT='PRESSED (AXPress) AXButton | Save | matches=2' \
  "$CU" --json clickel Demo Save --role AXButton --index 2
)
assert_contains "$clicked" '"ok":true' "clickel JSON reports success"
assert_contains "$clicked" '"index":2' "clickel JSON reports selector index"
pass "role/index click selector"

CLICLICK=/usr/bin/true CU_DIR="$TMP/state" "$CU" type --stdin --secret <<< "secret-value" >"$TMP/type.out" 2>"$TMP/type.err"
assert_contains "$(cat "$TMP/type.err")" "prefer --stdin" "secret warning is emitted"
pass "secret typing warning"

CLICLICK=/usr/bin/true CU_DIR="$TMP/state" "$CU" scroll top 2 >/dev/null
pass "keyboard scroll fallback"

print -r -- "all tests passed"
