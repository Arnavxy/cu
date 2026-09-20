# cu

cu is a small, local computer-use harness for macOS agents and automation
scripts. It gives an agent structured eyes and hands:

- Accessibility-tree inspection before screenshots.
- Semantic clicks by accessible name, role, and occurrence index.
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

Grant Accessibility to the terminal or agent process running cu under
System Settings -> Privacy & Security -> Accessibility. Grant Screen Recording
when using screenshots. Run:

  ./bin/cu doctor

The current implementation is designed for both Apple Silicon and Intel Macs.
Native and well-behaved Electron applications expose the best accessibility
trees; custom-rendered applications may require screenshot/coordinate fallback.

## Examples

  # Inspect only interactive controls, as compact text
  ./bin/cu tree "Safari" "Address"

  # Ask for all matching elements as JSON
  ./bin/cu --json tree "Notes" --all

  # Disambiguate duplicate labels
  ./bin/cu clickel "Notes" "Delete" --role AXButton --index 2

  # Keep secrets out of shell history and cu logs
  printf '%s' "$PASSWORD" | ./bin/cu type --stdin --secret

  # Use an actual mouse wheel
  ./bin/cu scroll down 4

  # Capture a screenshot (treat it as sensitive data)
  ./bin/cu shot before

JSON mode is available globally (cu --json ...) or on tree/clickel directly.
The output is intended to be easy for an agent wrapper to consume without
parsing human-oriented status text.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| CU_DIR | ~/.cu | Screenshot and log directory |
| CU_KEEP_SHOTS | 20 | Number of screenshots retained |
| CLICLICK | /opt/homebrew/bin/cliclick | Input backend |
| CU_OSASCRIPT | /usr/bin/osascript | AppleScript/JXA backend; useful for tests |

## Testing

Tests use mocked osascript and cliclick paths, so they do not click your
desktop:

  ./tests/test_cu.zsh

## Scope and safety

cu can read accessibility state, capture screens, type text, and control
applications with the permissions you grant it. Treat screenshots, accessibility
output, logs, and agent clients as potentially sensitive. Use type --stdin for
credentials and review agent permissions before connecting cu to an untrusted
client.

## Why another macOS computer-use tool?

There are larger MCP and multi-platform projects in this space. cu focuses on
being a transparent, dependency-light shell harness that can be embedded in
any agent loop or test runner. It is deliberately not an MCP server, model
wrapper, or background daemon; those can be layered on later.

## License

MIT. See LICENSE.

