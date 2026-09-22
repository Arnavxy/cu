---
name: cu-computer-use
description: Control and inspect native macOS applications with the local cu CLI when a task requires visible GUI interaction, app launching, clicking, typing, scrolling, screenshots, or verified UI actions. Prefer direct APIs, browser automation, or file operations when GUI interaction is not actually required.
---

# cu computer use

Use `cu` as the local macOS eyes-and-hands layer. It operates in logical screen points and supports native Accessibility semantics, local OCR, pointer and keyboard input, targeted scrolling, screenshots, and inexpensive visual verification.

## Before acting

- Resolve the executable with `command -v cu`. If missing, install it with `brew install Arnavxy/tap/cu` when installation is within the user's request; otherwise report that command.
- Record `cu --version` before relying on a recently added feature. `command -v` proves only that a binary exists; a `~/bin` or Homebrew copy may be older than the checkout. If working in the `cu` repository, compare it with `./bin/cu --version` and use the intended binary explicitly.
- Run `cu doctor` when permissions or input behavior are uncertain. Accessibility and Screen Recording permissions may require the user to enable macOS settings.
- Keep ordinary authorization boundaries. The existence of this skill does not authorize sending messages, publishing, purchasing, deleting data, entering secrets, or operating outside the user's requested scope.

## Efficient interaction loop

1. Activate an existing app with `cu open "App"` when needed.
2. Start with `cu observe "App" --json --max-elements N`, adding `--name TEXT` or `--role ROLE` when known. Filter the JSON locally to relevant names, roles, and safe read-only values instead of returning a large accessibility tree to the model. Values from editable or password-like controls are intentionally omitted.
3. Prefer a unique semantic element and immediately call `cu act e_N --snapshot s_ID --json`. Treat handles as snapshot-scoped; re-observe after meaningful UI changes.
4. Check the returned verification object, then observe the expected state or target text. Do not assume a reported click means the intended outcome occurred. For a known dialog/window transition, prefer `cu wait --window APP TITLE [--gone] [--timeout MS]` over a guessed sleep.
   `AXPress` means the Accessibility request was accepted, not necessarily that an app handler ran. `cu act` checks the target semantically before falling back to its fresh center, including when the surrounding window is animated. Treat `visual_activity: animated` with `changed: null` as indeterminate and re-observe the expected state.
5. Escalate only as needed:
   - `cu tree "App" FILTER` or `cu clickel "App" "Name" --role ROLE`
   - `cu clicktext "App" "Visible text"` for local OCR fallback
   - `cu shot -w "App"` or `cu shot NAME`, then `cu click X Y` for visual-only interfaces
6. For coordinate actions, prefer `cu click --window APP X Y` for screenshot/window-relative points or `cu click --app APP X Y` for guarded absolute points. Verify with a fresh observation, `cu color X Y`, or `cu diff BEFORE AFTER`.

For multi-step work, use `cu batch --stdin` to validate the complete script and keep the sequence in one process. Batch also accepts read verbs (`observe`, `find`, and `shot`), so an agent can finish `act → wait → observe` without another CLI startup. Clipboard `paste --stdin` is intentionally not a batch verb because batch owns stdin. Use `cu wait --element`, `cu wait --value`, `cu wait --stable`, or `cu wait --changed` instead of fixed sleeps when the expected state is expressible locally. `cu batch --help` lists the supported verbs.

Prefer semantic actions because they are faster, more stable, and cheaper than screenshot reasoning. Use screenshots only when Accessibility and OCR are insufficient. Some applications expose auxiliary windows before their main window; inspect `cu bounds`, `cu windows`, or a full-screen shot when a window capture is unexpectedly small.

## Input and scrolling

- Use `cu type "text"` for short input, but verify what arrived in custom, Electron, or DeviceHub/iOS surfaces. If characters drop, use `cu type --paced --interval 180 "text"`; it sends one character at a time with inter-key waits in one input call. Verify the field rather than retrying a whole string blindly.
- Use `cu paste --stdin` for fast multiline content on normal native/web fields; it restores the previous clipboard. Do not assume it reaches a DeviceHub/iOS surface—verify the resulting field first and fall back to paced typing.
- Use `cu type --stdin --secret` for user-authorized secrets so they are not placed in command arguments or action logs.
- Use `cu key return`, `cu combo cmd s`, pointer commands, and drag commands for ordinary input.
- Target scrolling explicitly when possible: `cu scroll --app "App" --at X Y down 4`.
- If an embedded surface such as DeviceHub ignores wheel events, use `cu drag` for direct manipulation. Verify the new visible region before continuing.
- If Return does not submit in a terminal, `cu combo ctrl m` is an equivalent fallback.

## Privacy and cleanup

- Screenshots can contain private data. Capture the smallest useful scope, inspect before sharing, and never publish unrelated windows, tabs, messages, credentials, or project details.
- `cu` stores screenshots under `~/.cu/shots` and automatically prunes old captures. Use `cu clean` only when removing all stored captures is intended.
- Avoid typing credentials into visible demos. Never include secret values in logs, screenshots, examples, or final responses.

Use `cu help` for the installed command reference and `cu log N` when diagnosing recent actions.

## Device Hub / iOS Simulator

On Xcode 27, the host app is `DeviceHub`, not `Simulator`. Start with `cu apps` or `cu windows` and use the exact running app name; for example, `cu observe "DeviceHub" --all --json` audits a running app's exposed Accessibility tree from outside the app. React Native may bridge modal/overlay controls as `AXGenericElement`, so interactive-only observation can return an empty tree even when `AXPress` still works. Use `--all` for dense DeviceHub overlays, count the bridged roles, and report the findings rather than inferring accessibility from pixels. DeviceHub often exposes hybrid/custom UI: prefer AX for inspection, but expect paste and wheel scrolling to need the fallbacks above.
