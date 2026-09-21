#!/bin/zsh
set -eu

ROOT="${0:A:h:h}"
CU="$ROOT/bin/cu"
TMP="${TMPDIR:-/tmp}/cu-test.$$.${RANDOM}"
mkdir -p "$TMP"
trap 'rm -rf "$TMP"' EXIT

chmod +x "$ROOT"/tests/fixtures/mock-*

pass() { print -r -- "ok - $1"; }
fail() { print -r -- "not ok - $1" >&2; exit 1; }
assert_contains() { [[ "$1" == *"$2"* ]] || fail "$3"; }

zsh -n "$CU" || fail "script parses"
pass "script parses"

assert_contains "$(CU_DIR="$TMP/state" "$CU" --version)" "cu 0.2.0" "version is reported"
pass "version command"

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
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_MOCK_OSASCRIPT_RESULT='PRESSED (AXPress) AXButton | Save | matches=2' \
  "$CU" --json clickel Demo Save --role AXButton --index 2
)
assert_contains "$clicked" '"ok":true' "clickel JSON reports success"
assert_contains "$clicked" '"index":2' "clickel JSON reports selector index"
pass "role/index click selector"

press_script="$TMP/press.applescript"
CU_DIR="$TMP/state" \
  CU_OSASCRIPT="$ROOT/tests/fixtures/mock-osascript" \
  CU_NATIVE="$TMP/no-native-helper" \
  CU_MOCK_OSASCRIPT_CAPTURE="$press_script" \
  CU_MOCK_OSASCRIPT_RESULT='PRESSED (AXPress) AXButton | Save | matches=1' \
  "$CU" clickel Demo Save >/dev/null
/usr/bin/osacompile -o "$TMP/press.scpt" "$press_script" || fail "generated AX press AppleScript compiles"
pass "generated AX press AppleScript compiles"

CLICLICK=/usr/bin/true CU_DIR="$TMP/state" "$CU" type --stdin --secret <<< "secret-value" >"$TMP/type.out" 2>"$TMP/type.err"
assert_contains "$(cat "$TMP/type.err")" "prefer --stdin" "secret warning is emitted"
pass "secret typing warning"

multiline_log="$TMP/multiline.log"
CU_DIR="$TMP/state" \
  CLICLICK="$ROOT/tests/fixtures/mock-cliclick" \
  CU_MOCK_CLICK_LOG="$multiline_log" \
  "$CU" type $'first\nsecond' >/dev/null
assert_contains "$(cat "$multiline_log")" 't:first kp:return t:second' "multiline type emits Return keys"
pass "multiline keyboard typing"

paste_result=$(
  print -rn -- $'first\nsecond' | \
    CU_DIR="$TMP/state" \
    CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
    "$CU" --json paste --stdin
)
assert_contains "$paste_result" '"ok":true' "paste returns JSON success"
assert_contains "$paste_result" '"pasted_chars":12' "paste reports character count"
pass "clipboard-safe multiline paste"

CLICLICK=/usr/bin/true CU_DIR="$TMP/state" "$CU" scroll top 2 >/dev/null
pass "keyboard scroll fallback"

fallback_tree=$(
  CU_DIR="$TMP/state" \
  CU_OSASCRIPT="$ROOT/tests/fixtures/mock-osascript" \
  CU_MOCK_OSASCRIPT_RESULT='ERR: AX unavailable' \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  "$CU" --json tree Demo --all
)
assert_contains "$fallback_tree" '"source":"ocr"' "tree reports OCR fallback"
assert_contains "$fallback_tree" '"name":"Save"' "OCR fallback returns visible text"
pass "AX to OCR tree fallback"

click_log="$TMP/click.log"
fallback_click=$(
  CU_DIR="$TMP/state" \
  CU_OSASCRIPT="$ROOT/tests/fixtures/mock-osascript" \
  CU_MOCK_OSASCRIPT_RESULT='ERR: AX unavailable' \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  CLICLICK="$ROOT/tests/fixtures/mock-cliclick" \
  CU_MOCK_CLICK_LOG="$click_log" \
  "$CU" clicktext Demo Save
)
assert_contains "$fallback_click" 'PRESSED (OCRClick)' "clicktext reports OCR click"
assert_contains "$(cat "$click_log")" 'c:150,250' "clicktext uses OCR screen coordinates"
pass "guarded OCR text click"

display_click_log="$TMP/display-click.log"
display_click=$(
  CU_DIR="$TMP/state" \
  CU_OSASCRIPT="$ROOT/tests/fixtures/mock-osascript" \
  CU_MOCK_OSASCRIPT_RESULT='ERR: AX unavailable' \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_MOCK_OCR_MODE=display-only \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  CLICLICK="$ROOT/tests/fixtures/mock-cliclick" \
  CU_MOCK_CLICK_LOG="$display_click_log" \
  "$CU" clicktext Demo File
)
assert_contains "$display_click" 'PRESSED (OCRClick)' "display OCR reports click"
assert_contains "$(cat "$display_click_log")" 'c:50,15' "display OCR can click app menu text"
pass "display OCR menu fallback"

observation=$(
  CU_DIR="$TMP/runtime-state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  "$CU" observe Demo --json
)
assert_contains "$observation" '"snapshot":"s_' "observe returns snapshot ID"
assert_contains "$observation" '"source":"native_ax"' "observe uses native AX"
assert_contains "$observation" '"id":"e_1"' "observe returns stable element handles"
snapshot=$(print -r -- "$observation" | sed -n 's/.*"snapshot":"\([^"]*\)".*/\1/p')
runtime_action=$(
  CU_DIR="$TMP/runtime-state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  "$CU" act e_1 --snapshot "$snapshot" --json
)
assert_contains "$runtime_action" '"action":"AXPress"' "act uses native AX action"
assert_contains "$runtime_action" '"verification":{"available":true,"changed":false' "act automatically verifies"
pass "observe/act runtime"

if stale=$(
  CU_DIR="$TMP/runtime-state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_MOCK_WINDOW_ID=99 \
  "$CU" act e_1 --snapshot "$snapshot" --json
); then
  fail "act rejects a stale window snapshot"
fi
assert_contains "$stale" '"code":"stale_snapshot"' "stale snapshot has structured error"
pass "stale snapshot rejection"

print -r -- "all tests passed"
