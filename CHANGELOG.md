# Changelog

All notable changes to this project are documented here.

## 0.2.2 - 2026-09-21

### Added

- A portable `cu-computer-use` Agent Skill with automatic invocation metadata,
  an efficient observe-act-verify workflow, fallback guidance, and privacy
  boundaries.
- README instructions for repository-scoped and global skill discovery.

### Changed

- `observe` now uses one native context call for window identity and AX traversal,
  avoiding redundant native process launches on the semantic fast path.
- Window matching now favors the Accessibility-focused window over same-app
  utility chrome, improving target accuracy for multi-window applications.
- Clipboard paste now waits for pasteboard propagation and gives delayed
  clipboard consumers a configurable handoff window before restoration.

## 0.2.1 - 2026-09-21

### Changed

- Coordinate actions now move to the target and settle before clicking, improving
  reliability for small and hover-sensitive controls.
- Wheel input is now emitted by the native helper at an explicit logical point.
- `scroll` can activate and target any application with `--app` and can address
  embedded surfaces precisely with `--at X Y`.
- Scroll results include their target application and point in JSON mode.

## 0.2.0 - 2026-09-21

### Added

- Native `AXUIElement` observation and actions through the Swift helper.
- `observe --json` snapshots with compact, stable element handles.
- `act` with snapshot age, process, window, target, and bounds validation.
- Automatic post-action verification using window identity and pixel fingerprints.
- CoreGraphics app/window discovery and logical multi-display coordinates.
- On-device Vision OCR fallback for hardened and nonstandard applications.
- Clipboard-safe `paste --stdin`, multiline keyboard input, and wheel scrolling.
- Mocked runtime tests and a reusable, non-personal Xcode demo.

### Security

- Snapshots use owner-only filesystem permissions and expire for action purposes.
- Editable and secure field values are omitted from native observations.
- OCR captures are temporary, while retained screenshots carry an explicit warning.

## 0.1.0

- Initial macOS computer-use command harness.
