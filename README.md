# cu

cu is a small, local computer-use harness for macOS agents and automation
scripts. It gives an agent structured eyes and hands:

- Accessibility-tree inspection before screenshots.
- Semantic clicks by accessible name, role, and occurrence index.
- CoreGraphics window discovery when hardened apps expose no AX windows.
- On-device Vision OCR and guarded visible-text clicks as the final fallback.
- Logical-point mouse and keyboard actions.
- Real CoreGraphics wheel scrolling.
- JSON output for machine clients.
- Local screenshots, verification helpers, and an action log.

cu is intentionally only the tool layer. It does not choose goals, call an LLM,
or run an agent loop.

## Requirements

- macOS 13 Ventura or newer.
- zsh, which ships with macOS.
- cliclick for pointer and keyboard input:

  brew install cliclick

- Xcode Command Line Tools, used to build the small native helper:

  xcode-select --install

Grant Accessibility to the terminal or agent process running cu under
System Settings -> Privacy & Security -> Accessibility. Grant Screen Recording
when using screenshots. Run:

  ./bin/cu doctor

Build and install both executables:

  ./scripts/install.zsh

This installs to `~/bin` by default. Set `CU_INSTALL_DIR` to choose another
directory. The implementation is designed for both Apple Silicon and Intel
Macs.

## Fallback model

cu takes the cheapest reliable path available:

1. Inspect or press an element through macOS Accessibility.
2. If AX cannot enumerate a window, locate it through CoreGraphics.
3. Capture only that window, recognize text locally with Apple Vision, and
   return logical screen coordinates.
4. For app-owned menu-bar text, search the display containing the app window.

The final click is rechecked against the current window or display bounds before
input is sent. Exact text is preferred over substring matches, and ambiguous
matches require `--index`. Multi-display coordinates come from CoreGraphics;
callers do not perform Retina scaling math.

## Examples

  # Inspect only interactive controls, as compact text
  ./bin/cu tree "Safari" "Address"

  # Ask for all matching elements as JSON
  ./bin/cu --json tree "Notes" --all

  # Disambiguate duplicate labels
  ./bin/cu clickel "Notes" "Delete" --role AXButton --index 2

  # OCR fallback for a custom UI that exposes no Accessibility element
  ./bin/cu clicktext "ExampleApp" "New Window"

  # Keep secrets out of shell history and cu logs
  printf '%s' "$PASSWORD" | ./bin/cu type --stdin --secret

  # Use an actual mouse wheel
  ./bin/cu scroll down 4

  # Capture a screenshot (treat it as sensitive data)
  ./bin/cu shot before

JSON mode is available globally (`cu --json ...`) or on `tree`, `clickel`, and
`clicktext` directly. A tree response includes `source: "ax"` or
`source: "ocr"`, so an agent can adjust its confidence or policy explicitly.
The output is intended to be easy for an agent wrapper to consume without
parsing human-oriented status text.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| CU_DIR | ~/.cu | Screenshot and log directory |
| CU_KEEP_SHOTS | 20 | Number of screenshots retained |
| CLICLICK | discovered from PATH | Input backend |
| CU_OSASCRIPT | /usr/bin/osascript | AppleScript/JXA backend; useful for tests |
| CU_NATIVE | sibling `cu-native` executable | CoreGraphics and Vision helper |
| CU_SCREENCAPTURE | /usr/sbin/screencapture | Screenshot backend; useful for tests |
| CU_SIPS | /usr/bin/sips | Image scaling backend; useful for tests |

## Testing

Tests mock AppleScript, screenshots, OCR, and pointer input, so they do not
click your desktop. They cover AX selectors, JSON, AX-to-OCR fallback,
window-relative coordinates, and display-level menu fallback:

  ./tests/test_cu.zsh

## Scope and safety

cu can read accessibility state, capture screens, type text, and control
applications with the permissions you grant it. Treat screenshots, accessibility
output, logs, and agent clients as potentially sensitive. Use type --stdin for
credentials and review agent permissions before connecting cu to an untrusted
client. Temporary OCR captures are deleted immediately; named screenshots are
retained locally and auto-pruned.

## Why another macOS computer-use tool?

There are larger MCP and multi-platform projects in this space. cu focuses on
being a transparent, token-aware harness that can be embedded in any agent loop
or test runner. It is a computer-use adapter, not a simulator: it controls the
real macOS session while leaving planning and policy to the calling agent. It is
deliberately not an MCP server, model wrapper, or background daemon; those can
be layered on later.

## License

MIT. See LICENSE.
