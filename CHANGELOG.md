# Changelog

All notable changes to this project are documented here.

## Unreleased

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
