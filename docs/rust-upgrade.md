# Upgrading to CU 0.5.0

This release upgrades the agent-facing CLI and MCP server. It does not package
the separate CU Voice application, an LLM, user accounts, or personal state.

## What changed

The shell command dispatcher, Python MCP process and Swift native/input runtime
are replaced by four compiled Rust executables: `cu`, `cu-mcp`, `cu-native` and
`cu-input`. Apple Accessibility, Vision, CoreGraphics and application scripting
remain the macOS integration layer. Fixed AppleScript/JXA calls and system tools
may still be used where macOS exposes functionality that way; rewriting the
caller does not replace those operating system APIs.

The native path supports bounded Accessibility reads, document text, snapshot
handles, semantic waits, guarded input and local OCR/image fallback. Existing
commands and snapshot/JSON contracts remain supported. Rust's native batches
validate the complete command sequence before dispatching input. The MCP server
is persistent and uses bounded newline-delimited JSON-RPC over stdio.

## Install or upgrade

```sh
brew update
brew upgrade Arnavxy/tap/cu
cu --version
cu doctor
```

For a first install, use `brew install Arnavxy/tap/cu`. Release archives provide
Apple Silicon and Intel builds. Extract the matching archive and run its
`scripts/install.zsh`; use `CU_INSTALL_DIR` to select the destination. Source
installation uses the same script but requires Rust and Xcode Command Line Tools.

Configure MCP clients to launch `cu-mcp` directly from its installed absolute
path. Older source configurations using `scripts/cu-mcp.py` have a deprecated
exec-only compatibility shim; Python is not part of the server implementation.
Restart an MCP client after replacing its running server. The `bin/cu` checkout
launcher is similarly a small build-location wrapper; installed `cu` is Rust.

All four executables should be upgraded together. Helper lookup resolves the
actual executable's siblings, including through a symlink. Existing `~/.cu`
state is retained. Stale live application snapshots must be observed again;
upgrading does not make old handles safe to replay. macOS may ask for permissions
for a new executable identity; this release does not reset permissions or change
privacy settings. Run `cu doctor` from the terminal/agent that will use it.

`cliclick` is no longer a required dependency. An explicit `CLICLICK` path is
retained for integrations and test fixtures; without it, CU uses its Rust input
helper. There is no silent fallback to an unrelated Homebrew input binary.

## Performance and limits

Rust removes shell/interpreter startup and orchestration subprocesses from the
hot path. It cannot remove macOS application/rendering/network latency. The
benchmark report distinguishes deterministic mocked runtime overhead from real
application latency. Assembly is not used: no measured bottleneck currently
justifies introducing architecture-specific code.

CU remains a tool for a calling agent, not an autonomous planner. Accessibility
coverage depends on the application; OCR and scoped screenshots remain useful
fallbacks. Verification reports observed effects and uncertainty, not a promise
that every arbitrary application action succeeds. Permissions, user input and
ambiguous targets retain their guards.
