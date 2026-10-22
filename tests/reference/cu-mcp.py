#!/usr/bin/env python3
"""Optional zero-dependency MCP stdio adapter for the cu CLI."""

import json
import os
import math
import signal
import tempfile
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CU = os.environ.get("CU_BIN") or shutil.which("cu") or str(ROOT / "bin" / "cu")


TOOLS = [
    {"name": "cu_observe", "description": "Observe a macOS app as bounded JSON. Set background to avoid stealing focus; it uses native AX then isolated window OCR if needed. Use since to return only semantic changes from a prior snapshot.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "background": {"type": "boolean"}, "since": {"type": "string"}, "all": {"type": "boolean"}, "role": {"type": "string"}, "name": {"type": "string"}, "max_elements": {"type": "integer", "minimum": 1}}, "required": ["app"]}},
    {"name": "cu_act", "description": "Act on a snapshot-scoped cu element. Set background for AX-only execution with no focus, screenshots, or pointer fallback.", "inputSchema": {"type": "object", "properties": {"element": {"type": "string"}, "snapshot": {"type": "string"}, "background": {"type": "boolean"}, "verify": {"type": "boolean", "default": True}}, "required": ["element", "snapshot"]}},
    {"name": "cu_audit", "description": "Audit a native Accessibility tree without screenshots: unnamed interactive controls, duplicate accessible names, and generic-element counts.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "max_issues": {"type": "integer", "minimum": 1}}, "required": ["app"]}},
    {"name": "cu_wait", "description": "Wait for a window, semantic element/value, stability, or visual change.", "inputSchema": {"type": "object", "properties": {"mode": {"type": "string", "enum": ["window", "element", "value", "stable", "changed"]}, "app": {"type": "string"}, "target": {"type": "string"}, "role": {"type": "string"}, "gone": {"type": "boolean"}, "timeout_ms": {"type": "integer"}}, "required": ["mode", "app"]}},
    {"name": "cu_batch", "description": "Run a guarded line-oriented cu action batch in one process.", "inputSchema": {"type": "object", "properties": {"script": {"type": "string"}}, "required": ["script"]}},
    {"name": "cu_shot", "description": "Capture a full screen or named app window to a local PNG.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "name": {"type": "string"}}}},
    {"name": "cu_click", "description": "Click logical macOS screen coordinates. Supply app to refuse clicks unless that app is frontmost; set window_relative when coordinates came from a window capture.", "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "app": {"type": "string"}, "window_relative": {"type": "boolean"}}, "required": ["x", "y"]}},
    {"name": "cu_clickel", "description": "Click a native Accessibility element by app and accessible name, with role/index disambiguation.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "name": {"type": "string"}, "role": {"type": "string"}, "index": {"type": "integer", "minimum": 0}}, "required": ["app", "name"]}},
    {"name": "cu_type", "description": "Type text into the focused field; use paced for fragile Simulator/custom fields.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}, "paced": {"type": "boolean"}, "interval_ms": {"type": "integer", "minimum": 0}, "secret": {"type": "boolean"}}, "required": ["text"]}},
]


# Bounds apply before invoking the shared CLI, including for untrusted MCP clients.
MAX_OUTPUT = 2 * 1024 * 1024
TOOLS[0]["inputSchema"]["properties"]["native_only"] = {"type": "boolean", "default": False}
TOOLS[0]["description"] = "Observe bounded native Accessibility JSON, Set native_only=true to prohibit screenshot/OCR fallback; the existing automatic local OCR behavior remains the default. Use background and since to reduce interruption and repeated context."
for tool in TOOLS:
    schema = tool["inputSchema"]
    schema["additionalProperties"] = False
    for key, prop in schema["properties"].items():
        if prop["type"] == "string":
            prop["maxLength"] = 65536 if key in ("text", "script") else 1024
        if key in ("max_elements", "max_issues"):
            prop["maximum"] = 1000
        if key == "timeout_ms":
            prop.update(minimum=1, maximum=60000)
        if key == "interval_ms":
            prop["maximum"] = 1000


def validate(name, arguments):
    tool = next((t for t in TOOLS if t["name"] == name), None)
    if tool is None or not isinstance(arguments, dict):
        raise ValueError("Unknown tool or invalid arguments")
    schema = tool["inputSchema"]
    if set(arguments) - set(schema["properties"]) or any(k not in arguments for k in schema.get("required", [])):
        raise ValueError("Missing or unsupported arguments")
    types = {"string": (str,), "boolean": (bool,), "integer": (int,), "number": (int, float)}
    for key, value in arguments.items():
        prop = schema["properties"][key]
        if type(value) not in types[prop["type"]]:
            raise ValueError("Invalid argument type: " + key)
        if isinstance(value, str) and (len(value) > prop["maxLength"] or "\x00" in value):
            raise ValueError("Invalid argument length or NUL: " + key)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            if not math.isfinite(value) or value < prop.get("minimum", -1e9) or value > prop.get("maximum", 1e9):
                raise ValueError("Argument outside supported range: " + key)
        if "enum" in prop and value not in prop["enum"]:
            raise ValueError("Invalid argument choice: " + key)
    if name == "cu_click" and arguments.get("window_relative") and not arguments.get("app"):
        raise ValueError("window_relative requires app")


def invoke(args, stdin=None):
    # Temporary files avoid unbounded memory capture. Kill descendants on timeout;
    # do not retry actions, since a timed-out mutation may already have happened.
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as source:
        source.write((stdin or "").encode())
        source.seek(0)
        try:
            proc = subprocess.Popen([CU, *args], stdin=source, stdout=output,
                                    stderr=output, start_new_session=True)
        except OSError:
            return 127, json.dumps({"ok": False, "error": {"code": "cli_unavailable"}})
        try:
            proc.wait(timeout=65)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
            return 124, json.dumps({"ok": False, "error": {"code": "cli_timeout", "message": "Execution timed out. Inspect current state before retrying; actions may have completed."}})
        output.seek(0)
        data = output.read(MAX_OUTPUT + 1)
        if len(data) > MAX_OUTPUT:
            return 1, json.dumps({"ok": False, "error": {"code": "output_limit", "message": "Output exceeded the limit; narrow the observation. Actions were not retried."}})
        return proc.returncode, data.decode("utf-8", errors="replace").strip()


def call_tool(name, arguments):
    arguments = {} if arguments is None else arguments
    validate(name, arguments)
    if name == "cu_observe":
        args = ["--json", "observe", arguments["app"]]
        if arguments.get("native_only", False):
            args.append("--native-only")
        if arguments.get("background"):
            args.append("--background")
        if arguments.get("since"):
            args += ["--since", arguments["since"]]
        if arguments.get("all"):
            args.append("--all")
        if arguments.get("role"):
            args += ["--role", arguments["role"]]
        if arguments.get("name"):
            args += ["--name", arguments["name"]]
        if arguments.get("max_elements") is not None:
            args += ["--max-elements", str(arguments["max_elements"])]
        return invoke(args)
    if name == "cu_act":
        args = ["--json", "act", arguments["element"], "--snapshot", arguments["snapshot"]]
        if arguments.get("background"):
            args.append("--background")
        if arguments.get("verify", True) is False:
            args.append("--no-verify")
        return invoke(args)
    if name == "cu_audit":
        args = ["--json", "audit", arguments["app"]]
        if arguments.get("max_issues") is not None:
            args += ["--max-issues", str(arguments["max_issues"])]
        return invoke(args)
    if name == "cu_wait":
        mode = arguments.get("mode", "window")
        args = ["--json", "wait"]
        if mode == "window":
            args += ["--window", arguments["app"], arguments.get("target", "")]
        else:
            args += [f"--{mode}", arguments["app"]]
            if mode in ("element", "value"):
                args.append(arguments.get("target", ""))
        if arguments.get("role"):
            args += ["--role", arguments["role"]]
        if arguments.get("gone"):
            args.append("--gone")
        if arguments.get("timeout_ms") is not None:
            args += ["--timeout", str(arguments["timeout_ms"])]
        return invoke(args)
    if name == "cu_batch":
        return invoke(["--json", "batch", "--stdin"], arguments["script"])
    if name == "cu_shot":
        args = ["shot"]
        if arguments.get("app"):
            args += ["-w", arguments["app"]]
        if arguments.get("name"):
            args.append(arguments["name"])
        return invoke(args)
    if name == "cu_click":
        args = ["--json", "click"]
        if arguments.get("app"):
            args += ["--window" if arguments.get("window_relative") else "--app", arguments["app"]]
        args += [str(arguments["x"]), str(arguments["y"])]
        return invoke(args)
    if name == "cu_clickel":
        args = ["--json", "clickel", arguments["app"], arguments["name"]]
        if arguments.get("role"):
            args += ["--role", arguments["role"]]
        if arguments.get("index") is not None:
            args += ["--index", str(arguments["index"])]
        return invoke(args)
    if name == "cu_type":
        args = ["--json", "type"]
        if arguments.get("secret"):
            args.append("--secret")
        if arguments.get("paced"):
            args.append("--paced")
        if arguments.get("interval_ms") is not None:
            args += ["--interval", str(arguments["interval_ms"])]
        args.append("--stdin")
        return invoke(args, arguments["text"])
    raise ValueError(f"unknown tool: {name}")


def reply(message_id, result=None, error=None):
    payload = {"jsonrpc": "2.0", "id": message_id}
    payload["error" if error else "result"] = error or result
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def main():
    for raw_line in sys.stdin:
        line = raw_line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            reply(None, error={"code": -32700, "message": "Parse error"})
            continue
        if not isinstance(request, dict) or not isinstance(request.get("method"), str):
            reply(None, error={"code": -32600, "message": "Invalid request"})
            continue
        method = request.get("method")
        request_id = request.get("id")
        if method == "notifications/initialized":
            continue
        if request_id is None:
            continue
        if method == "initialize":
            reply(request_id, {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "cu", "version": "0.4.1"}})
        elif method == "tools/list":
            reply(request_id, {"tools": TOOLS})
        elif method == "tools/call":
            try:
                params = request.get("params")
                if not isinstance(params, dict) or not isinstance(params.get("name"), str):
                    raise ValueError("Invalid tool call parameters")
                code, output = call_tool(params["name"], params.get("arguments"))
                result = {"content": [{"type": "text", "text": output}], "isError": code != 0}
                try:
                    structured = json.loads(output)
                    if isinstance(structured, dict):
                        result["structuredContent"] = structured
                except json.JSONDecodeError:
                    pass
                reply(request_id, result)
            except ValueError as exc:
                reply(request_id, error={"code": -32602, "message": str(exc)})
            except Exception:
                reply(request_id, error={"code": -32603, "message": "Internal tool error"})
        else:
            reply(request_id, error={"code": -32601, "message": f"method not found: {method}"})


if __name__ == "__main__":
    main()
