# cu

[![Tests](https://github.com/Arnavxy/cu/actions/workflows/test.yml/badge.svg)](https://github.com/Arnavxy/cu/actions/workflows/test.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](#requirements)

**A fast, token-aware computer-use runtime for macOS agents.**

`cu` observes the real macOS desktop, exposes interactive UI elements as compact
JSON, acts on snapshot-scoped handles, and verifies the result. It gives an
agent reliable “eyes and hands” without requiring a full screenshot for every
step.

## Install with Homebrew

```bash
brew install Arnavxy/tap/cu
cu doctor
```

Requires macOS 13 Ventura or newer. Grant Accessibility to the terminal or
agent running `cu`; screenshot and OCR commands also require Screen Recording.

## Quick start

```bash
observation="$(cu observe "Xcode" --json)"
snapshot="$(jq -r '.snapshot' <<< "$observation")"
target="$(jq -r '.elements[] | select(.name == "Clone…") | .id' <<< "$observation")"

cu act "$target" --snapshot "$snapshot" --json
```

```json
{
  "ok": true,
  "action": "AXPress",
  "source": "native_ax",
  "verification": {"available": true, "changed": true, "window_state": "changed"}
}
```

## Agent skill

This repository ships an auto-discoverable Agent Skill at
`.agents/skills/cu-computer-use`. Codex can invoke `$cu-computer-use`
explicitly or select it automatically when a task requires native macOS GUI
interaction. The skill teaches agents to use the token-efficient semantic path
first, verify consequential actions, and fall back to OCR or screenshots only
when necessary.

Codex discovers the skill automatically while working inside this repository.
To install it globally from a clone:

```bash
mkdir -p "$HOME/.agents/skills"
cp -R .agents/skills/cu-computer-use "$HOME/.agents/skills/"
```

Alternatively, ask an agent with the skill installer to install
`cu-computer-use` from
`https://github.com/Arnavxy/cu/tree/main/.agents/skills/cu-computer-use`.
Restart an existing agent session if it does not detect the new skill.

## What cu is

`cu` is a local command-line runtime for computer-use agents and desktop
automation on macOS. It controls the user's real session through native
Accessibility APIs, CoreGraphics, Apple Vision, and guarded keyboard/mouse
input.

It is **not** a virtual computer, an autonomous agent, an LLM wrapper, or a
background service. The caller chooses the goal and the next action; `cu`
supplies observation, input, safety checks, and feedback. In that sense it
simulates human computer use against real applications rather than simulating
an operating system.

## Why it exists

Screenshot-only computer use is expensive and fragile. Traditional macOS
automation also fails on some hardened or nonstandard applications. `cu` uses
the cheapest reliable signal first and falls back only when necessary:

```text
native AX semantics
        │ unavailable
        ▼
CoreGraphics window discovery → window-only capture → on-device Vision OCR
        │
        ▼
guarded action → pixel/window verification → structured JSON result
```

This design keeps observations small, avoids Retina-coordinate math at the
call site, and still works when System Events reports no application windows.

## Capabilities

| Capability | What it provides |
| --- | --- |
| Observe | Native `AXUIElement` traversal with role, accessible name, safe read-only value, bounds, actions, and compact element IDs |
| Act | Snapshot-scoped semantic actions such as `AXPress`, with OCR click fallback |
| Guard | PID, window, element path, role, name, bounds, and snapshot-age validation before input |
| Verify | Post-action state reporting with semantic checks, animation detection, and coordinate fallback |
| Find windows | CoreGraphics discovery for apps hidden from AppleScript/System Events |
| Read visible text | On-device Vision OCR, limited to the target window whenever possible |
| Input | Click, double-click, right-click, drag, keys, shortcuts, multiline typing, clipboard-safe paste, and real wheel scrolling |
| Inspect | Accessibility trees, screenshots, pixel colors, window bounds, screen diffs, and action logs |
| Integrate | Machine-readable JSON, deterministic selectors, environment overrides, and mocked tests |

By default, `observe` returns interactive controls only. `--all` includes the
surrounding labels and other named elements when the agent needs more context.

## Build from source

### Requirements

- macOS 13 Ventura or newer
- Xcode Command Line Tools
- [`cliclick`](https://github.com/BlueM/cliclick) for pointer and keyboard input

```bash
xcode-select --install
brew install cliclick
git clone https://github.com/Arnavxy/cu.git
cd cu
./scripts/install.zsh
```

The installer builds the optimized Swift helper and installs `cu` and
`cu-native` to `~/bin`. Override the destination with `CU_INSTALL_DIR`.

Grant Accessibility to the terminal or agent process under **System Settings →
Privacy & Security → Accessibility**. Screenshot and OCR commands also need
Screen Recording permission. Then check the installation:

```bash
cu doctor
```

## Agent workflow

### 1. Observe

```bash
cu observe "Notes" --json
```

The response contains application/window identity and stable IDs such as
`e_1`. The private backing snapshot is stored under `~/.cu/snapshots` with
owner-only permissions.

### 2. Choose

The calling agent selects an element from its semantic role, name, bounds, and
available actions. No planning model is built into `cu`.

### 3. Act and verify

```bash
cu act e_1 --snapshot s_1789971554_19537 --json
```

Before acting, `cu` confirms that the original process, window, and target are
still valid. It safely translates moved-window coordinates and rejects changed
or ambiguous state. After the action, it reports whether the window changed,
stayed the same, switched, or closed.

Handles are deliberately short-lived. Snapshots expire after 120 seconds by
default and cannot be reused against a different process or window.

## Command reference

| Command | Purpose |
| --- | --- |
| `cu observe "App" --json` | Create a semantic snapshot with stable element handles |
| `cu act e_N --snapshot s_ID --json` | Act on a handle and verify the outcome |
| `cu tree "App" [filter] [--all]` | Print native accessible controls, names, and logical coordinates |
| `cu clickel "App" "Name" [--role ROLE] [--index N]` | Perform a named Accessibility action |
| `cu clicktext "App" "Text" [--index N]` | Click visible text through local OCR |
| `cu shot [name]` / `cu shot -w "App"` | Capture the display or one application window |
| `cu click`, `dclick`, `rclick`, `move`, `drag` | Use logical-point pointer input |
| `cu type`, `paste --stdin`, `key`, `combo` | Send keyboard input or clipboard-safe multiline text |
| `cu scroll [--app "App"] [--at X Y] up\|down\|top\|bottom [n]` | Target wheel or navigation scrolling at an application and point |
| `cu wait --element APP NAME` / `--value APP VALUE` | Wait for semantic state |
| `cu wait --stable APP` / `--changed APP` | Wait for visual state transitions |
| `cu batch --stdin` | Validate a complete script, then run actions and reads through one process |
| `cu color X Y`, `cu diff before after` | Verify visual state without image-model tokens |
| `cu apps`, `open`, `bounds` | Inspect and activate application context |
| `cu doctor`, `log`, `clean` | Diagnose, inspect history, and remove screenshots |

Run `cu help` for the complete local usage guide.

## Selection and fallback behavior

For semantic actions, exact accessible-name matches win over substring matches.
Use `--role` and `--index` when labels repeat. `clickel` tries native AX first,
then the legacy bridge, then local OCR. `observe` uses native AX and falls back
to OCR only when AX exposes no named elements.

All public coordinates are logical macOS points. Multi-display origins and
Retina scaling are resolved internally through CoreGraphics.

Coordinate clicks deliberately use `move → settle → click` instead of a bare
click event. This adds 35 ms by default and prevents small controls from missing
hover, focus, or hit-test updates. Target wheel input explicitly when the app
contains an embedded surface such as a simulator, canvas, or remote desktop:

```bash
cu scroll --app "DeviceHub" --at 640 500 down 4
```

With `--app` and no `--at`, `cu` targets the center of the front application
window. For touch-style content that does not accept wheel events, use `cu drag`
to perform the same direct manipulation a user would.

### Batch execution

`batch` keeps the agent hot path in one process and can end with a read:

```sh
printf '%s\n' \
  'open "Calculator"' \
  'wait --window "Calculator" "Calculator" --timeout 1000' \
  'observe "Calculator" --json' | cu batch --stdin
```

The complete input is syntax-validated before any action is sent. Supported
verbs are `click`, `dclick`, `rclick`, `move`, `drag`, `type`, `paste`, `key`,
`combo`, `scroll`, `clickel`, `clicktext`, `open`, `wait`, `act`, `observe`,
`find`, and `shot`. This preflight cannot predict an app changing state during
execution, so semantic waits and snapshot guards still apply.

## Performance model

- Interactive-only native traversal is the default to minimize latency and JSON size.
- `observe` obtains the focused window, identity, and Accessibility tree in one native call; it waits only when a newly activated app has not published a window yet.
- OCR runs locally and only as a fallback.
- Captures are scoped to the target window when possible.
- Verification uses a downsampled pixel fingerprint rather than returning another image.
- `paste --stdin` sends large or multiline input in one operation and restores the prior clipboard contents.

Actual latency depends on the target application's accessibility tree and the
machine. Consumers should measure their own workflows rather than assume fixed
timings.

### Optional MCP adapter

The core install stays CLI-only. Agents that speak MCP can install the
dependency-free adapter separately:

```bash
./scripts/install-mcp.zsh
```

It exposes `observe`, `act`, `wait`, `batch`, and `shot` by invoking the same
`cu` binary, so both surfaces share behavior without bundling a model, browser
engine, or background service.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `CU_DIR` | `~/.cu` | Private snapshots, screenshots, and action log |
| `CU_KEEP_SHOTS` | `20` | Number of named screenshots retained |
| `CU_SNAPSHOT_TTL` | `120` | Maximum actionable snapshot age in seconds |
| `CU_VERIFY_DELAY` | `0.20` | UI settling delay before verification |
| `CU_CLICK_SETTLE_MS` | `35` | Delay between pointer movement and coordinate click |
| `CU_WINDOW_READY_DELAY` | `0.15` | Maximum seconds to wait for a newly activated app window |
| `CU_WINDOW_READY_POLL` | `0.03` | Native window readiness polling interval in seconds |
| `CU_PASTE_READY_DELAY` | `0.02` | Clipboard propagation delay before Cmd-V |
| `CU_PASTE_RESTORE_DELAY` | `0.35` | Delay before restoring the caller's clipboard after Cmd-V |
| `CU_TYPE_INTERVAL_MS` | `180` | Inter-key delay used by `cu type --paced` for fragile input surfaces |
| `CLICLICK` | discovered from `PATH` | Input backend |
| `CU_NATIVE` | sibling `cu-native` | Native AX/CoreGraphics/Vision helper |
| `CU_OSASCRIPT` | `/usr/bin/osascript` | Legacy bridge and test override |
| `CU_SCREENCAPTURE` | `/usr/sbin/screencapture` | Capture backend and test override |
| `CU_SIPS` | `/usr/bin/sips` | Image-scaling backend and test override |

## Development

```bash
./tests/test_cu.zsh
./tests/benchmark.zsh
zsh -n bin/cu scripts/*.zsh tests/*.zsh
./scripts/build-native.zsh
```

Tests use mocked Accessibility, OCR, screenshot, and input backends, so the test
suite does not click the developer's desktop. A generic Xcode demonstration is
available at `./scripts/demo.zsh`; it intentionally contains no personal data.

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and
[CHANGELOG.md](CHANGELOG.md).

## Security and limitations

Computer-use tooling is powerful. `cu` can inspect accessibility state, capture
screens, and control applications with the permissions granted to its calling
process. Treat snapshots, screenshots, logs, and downstream agent clients as
sensitive.

Native observations omit values from editable and secure text fields. Use
`type --stdin --secret` for credentials; it avoids putting the secret in shell
history or the action log. Temporary OCR captures are deleted immediately.

Current limitations:

- macOS only; tested behavior varies with application accessibility quality.
- Canvas, game, remote-desktop, and heavily custom UIs may require OCR or raw input.
- Some embedded or touch-style surfaces ignore wheel events and require a drag gesture.
- macOS prevents automation of secure input and other protected system surfaces.
- Pixel verification establishes that the visible window changed, not that the user's high-level goal succeeded. `AXPress` means the Accessibility request was accepted; `cu act` now checks whether the target's semantic role/name/path changed before deciding that. If the target is still identical, it can use a fresh coordinate fallback even when the surrounding window is animated. Animated surfaces report `visual_activity: animated` and leave `changed` indeterminate when semantic verification also cannot prove an effect.
- Native AX and Screen Recording still require explicit macOS privacy permissions.

## License

MIT. See [LICENSE](LICENSE).
