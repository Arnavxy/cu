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

assert_contains "$(CU_DIR="$TMP/state" "$CU" --version)" "cu 0.2.3" "version is reported"
pass "version command"

help="$("$CU" --help)"
assert_contains "$help" "--json" "help documents JSON mode"
pass "help documents JSON mode"

windows=$( \
  CU_DIR="$TMP/state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  "$CU" windows Demo
)
assert_contains "$windows" $'Demo\tDemo Window' "windows exposes native CoreGraphics records"
pass "native window listing"

window_wait=$( \
  CU_DIR="$TMP/state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  "$CU" wait --window Demo 'Demo Window' --timeout 10
)
assert_contains "$window_wait" 'window ready: Demo — Demo Window' "condition wait observes native window state"
pass "condition-based window wait"

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

delete_log="$TMP/delete.log"
CU_DIR="$TMP/state" \
  CLICLICK="$ROOT/tests/fixtures/mock-cliclick" \
  CU_MOCK_CLICK_LOG="$delete_log" \
  "$CU" combo cmd backspace >/dev/null
assert_contains "$(cat "$delete_log")" 'kd:cmd kp:delete ku:cmd' "backspace shortcut maps to cliclick delete"
pass "backspace key compatibility"

if bad_key=$(CU_DIR="$TMP/state" "$CU" --json key 2>&1); then
  fail "key without an argument is rejected"
fi
assert_contains "$bad_key" 'usage: cu key KEY' "key reports a useful missing-argument error"
if bad_combo=$(CU_DIR="$TMP/state" "$CU" --json combo cmd 2>&1); then
  fail "combo without a key is rejected"
fi
assert_contains "$bad_combo" 'usage: cu combo MODIFIER... KEY' "combo reports a useful missing-key error"
pass "keyboard argument validation"

if bad_click=$(CU_DIR="$TMP/state" "$CU" --json click nope 20 2>&1); then
  fail "non-numeric click coordinates are rejected"
fi
assert_contains "$bad_click" 'coordinates must be numeric logical points' "click validates coordinates before input"
pass "coordinate argument validation"

if bad_bounds=$(CU_DIR="$TMP/state" "$CU" --json bounds 2>&1); then
  fail "bounds without an app is rejected"
fi
assert_contains "$bad_bounds" 'usage: cu bounds' "bounds reports a useful missing-app error"
if bad_scroll=$(CU_DIR="$TMP/state" CLICLICK=/usr/bin/true "$CU" --json scroll down 1 extra 2>&1); then
  fail "scroll with extra arguments is rejected"
fi
assert_contains "$bad_scroll" 'usage: cu scroll' "scroll reports a useful extra-argument error"
pass "context command validation"

pointer_log="$TMP/pointer.log"
CU_DIR="$TMP/state" \
  CLICLICK="$ROOT/tests/fixtures/mock-cliclick" \
  CU_MOCK_CLICK_LOG="$pointer_log" \
  CU_CLICK_SETTLE_MS=25 \
  "$CU" click 10 20 >/dev/null
assert_contains "$(cat "$pointer_log")" 'm:10,20 w:25 c:10,20' "click moves and settles before pressing"
pass "settled coordinate click"

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

scroll_log="$TMP/scroll-native.log"
scroll_result=$(
  CU_DIR="$TMP/state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_MOCK_NATIVE_LOG="$scroll_log" \
  "$CU" --json scroll --app Demo --at 150 250 down 3
)
assert_contains "$(cat "$scroll_log")" 'scroll 150 250 -1 3' "scroll is posted at the target point"
assert_contains "$scroll_result" '"point":[150,250]' "scroll reports its target point"
assert_contains "$scroll_result" '"app":"Demo"' "scroll reports its target app"
pass "targeted native scrolling"

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
assert_contains "$(cat "$click_log")" 'm:150,250 w:35 c:150,250' "clicktext settles on OCR screen coordinates"
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

observe_log="$TMP/observe-native.log"
observation=$(
  CU_DIR="$TMP/runtime-state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_MOCK_NATIVE_LOG="$observe_log" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  "$CU" observe Demo --json
)
assert_contains "$observation" '"snapshot":"s_' "observe returns snapshot ID"
assert_contains "$observation" '"source":"native_ax"' "observe uses native AX"
assert_contains "$observation" '"id":"e_1"' "observe returns stable element handles"
assert_contains "$(cat "$observe_log")" 'context Demo --interactive' "observe fetches window and AX data in one native call"
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

no_verify_action=$( \
  CU_DIR="$TMP/runtime-state" \
  CU_NATIVE="$ROOT/tests/fixtures/mock-native" \
  CU_SCREENCAPTURE="$ROOT/tests/fixtures/mock-screencapture" \
  CU_SIPS="$ROOT/tests/fixtures/mock-sips" \
  "$CU" act e_1 --snapshot "$snapshot" --no-verify --json
)
assert_contains "$no_verify_action" '"verification":{"available":false,"changed":null' "no-verify skips verification work"
pass "no-verify action path"

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
