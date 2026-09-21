#!/bin/zsh
set -eu

ROOT="${0:A:h:h}"
CU="$ROOT/bin/cu"
NATIVE="$ROOT/bin/cu-native"
APP="${1:-Xcode}"
TARGET="${CU_DEMO_TARGET:-Clone…}"

sleep "${CU_DEMO_DELAY:-0}"
print -n -- $'\e]0;cu — native macOS computer use\a'

blue=$'\e[38;5;81m'
green=$'\e[38;5;84m'
yellow=$'\e[38;5;220m'
dim=$'\e[2m'
bold=$'\e[1m'
reset=$'\e[0m'

section() {
  print
  print -r -- "${blue}${bold}$1${reset}"
}

command_line() {
  print -r -- "${dim}\$${reset} ${bold}$1${reset}"
  sleep 0.8
}

print -n -- $'\e[2J\e[H'
print -r -- "${blue}${bold}cu${reset}  ${dim}a token-aware computer-use runtime for macOS agents${reset}"
print -r -- "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
print
print -r -- "Challenge: open Xcode's Clone dialog when AppleScript sees no windows."
sleep 1.4

section "1  The usual AppleScript bridge is blind"
command_line "osascript → count $APP windows"
old_count=$(/usr/bin/osascript -e "tell application \"System Events\" to tell process \"$APP\" to count of windows" 2>/dev/null || print -r -- "error")
print -r -- "${yellow}System Events sees: ${old_count} windows${reset}"
sleep 1.5

section "2  Observe through native AXUIElement"
command_line "cu observe $APP --json"
state=$("$CU" observe "$APP" --json)
/usr/bin/osascript -e 'tell application "Terminal" to activate' >/dev/null 2>&1 || true
snapshot=$(print -r -- "$state" | /usr/bin/jq -r '.snapshot')
element=$(print -r -- "$state" | /usr/bin/jq -r --arg target "$TARGET" '.elements[] | select(.name == $target) | .id' | head -1)
print -r -- "$state" | /usr/bin/jq --arg target "$TARGET" '{snapshot, source, app, window, discovered_elements:(.elements|length), target:(.elements[]|select(.name==$target))}'
sleep 2.2

section "3  Act on the snapshot handle—not a coordinate"
command_line "cu act $element --snapshot $snapshot --json"
result=$("$CU" act "$element" --snapshot "$snapshot" --json)
print -r -- "$result" | /usr/bin/jq .
sleep 1.5

"$NATIVE" activate "$APP" >/dev/null 2>&1 || true
sleep 2.0
/usr/bin/osascript -e 'tell application "Terminal" to activate' >/dev/null 2>&1 || true
sleep 0.5

print
print -r -- "${green}${bold}✓ Native semantic action completed and verified${reset}"
print -r -- "${dim}No screenshot sent to a model. No raw coordinate guessed.${reset}"
print
print -r -- "${blue}github.com/Arnavxy/cu${reset}"
sleep 3
