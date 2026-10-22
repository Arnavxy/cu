#!/bin/zsh
# Integration regressions for the Rust dispatcher. All desktop operations are
# mocked; forbidden helpers prove these routes cannot silently fall back.
set -eu
ROOT="${0:A:h:h}"
CU="${CU_TEST_BIN:-$ROOT/cu-runtime/target/debug/cu}"
python3 - "$ROOT" "$CU" <<'PY'
import json
import os
import pathlib
import subprocess
import sys
import tempfile

root, binary = sys.argv[1:]
with tempfile.TemporaryDirectory(prefix="cu-rust-native-") as directory:
    state = pathlib.Path(directory)
    log = state / "native.log"
    clicks = state / "click.log"
    env = dict(os.environ, CU_DIR=directory,
               CU_NATIVE=root + "/tests/fixtures/mock-native",
               CU_MOCK_NATIVE_LOG=str(log),
               CU_LEGACY_BIN="/nonexistent/cu-legacy-disabled",
               CU_SCREENCAPTURE="/nonexistent/no-captures",
               CLICLICK="/nonexistent/no-pointer")

    def run(*args, code=0, extra=None, script=None):
        completed = subprocess.run([binary, "--json", *args],
                                   env={**env, **(extra or {})}, input=script,
                                   text=True, capture_output=True, timeout=10)
        assert completed.returncode == code, (args, completed.stdout, completed.stderr)
        return json.loads(completed.stdout)

    first = run("observe", "Demo", "--background", "--native-only")
    delta = run("observe", "Demo", "--background", "--native-only",
                "--since", first["snapshot"])
    assert delta["elements"] == [] and delta["delta"]["unchanged"] == 2
    filtered = run("observe", "Demo", "--background", "--native-only",
                   "--role", "AXButton", "--name", "Save", "--max-elements", "1")
    assert len(filtered["elements"]) == 1
    action = run("act", "e_1", "--snapshot", first["snapshot"],
                 "--background", "--native-only")
    verification = action["verification"]
    assert verification["available"] is True
    assert verification["changed"] is False  # an unchanged tree isn't claimed as a visible effect
    assert verification["semantic_effect"] == "none"
    assert verification["coordinate_fallback"] == "none"
    recorded = log.read_text()
    assert "activate " not in recorded and "fingerprint" not in recorded and "ocr " not in recorded
    print("ok - native-only semantic verification needs no pixels, pointer, focus or legacy runtime")

    stale = run("act", "e_1", "--snapshot", first["snapshot"],
                "--background", "--native-only", code=1,
                extra={"CU_MOCK_WINDOW_ID": "99"})
    assert stale["error"]["code"] == "stale_snapshot"
    gone = run("wait", "--element", "Demo", "Save", "--gone", "--timeout", "20",
               code=1, extra={"CU_MOCK_NATIVE_AX_UNAVAILABLE": "1"})
    assert gone["ok"] is False
    print("ok - stale windows and failed inspection never become successful actions or disappearance")

    for sparse in ["CU_MOCK_NATIVE_AX_UNAVAILABLE", "CU_MOCK_TEAMS_CHROME_ONLY", "CU_MOCK_BROWSER_SPARSE"]:
        failed = run("observe", "Demo", "--background", "--native-only", code=1,
                     extra={sparse: "1"})
        assert failed["error"]["code"] == "native_ax_unavailable"
    print("ok - sparse app/browser Accessibility fails explicitly without taking a screenshot")

    batch = run("batch", "--stdin", script="observe Demo --background --native-only\nwait --element Demo Save --timeout 100\n")
    assert batch["ok"] and len(batch["actions"]) == 2
    for invalid in ["unknown-command 3", "observe Demo --invalid", "act e_1 --snapshot bad"]:
        failed = run("batch", "--stdin", code=1, script="click 10 20\n" + invalid + "\n",
                     extra={"CLICLICK": root + "/tests/fixtures/mock-cliclick", "CU_MOCK_CLICK_LOG": str(clicks)})
        assert failed["ok"] is False
        assert not clicks.exists() or not clicks.read_text()
    print("ok - native batch runs in Rust and validates the entire script before input")

    ocr = run("observe", "Demo", "--background", extra={
        "CU_MOCK_NATIVE_AX_UNAVAILABLE": "1",
        "CU_SCREENCAPTURE": root + "/tests/fixtures/mock-screencapture",
        "CU_SIPS": root + "/tests/fixtures/mock-sips"})
    assert ocr["source"] == "vision_ocr"
    failed = run("act", "e_1", "--snapshot", ocr["snapshot"], "--background", code=1)
    assert failed["error"]["code"] == "background_action_unavailable"
    print("ok - explicit OCR fallback survives migration but cannot silently move a background pointer")
PY
