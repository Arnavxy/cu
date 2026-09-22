#!/usr/bin/env python3
"""Optional zero-dependency MCP stdio adapter for the cu CLI."""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CU = os.environ.get("CU_BIN") or shutil.which("cu") or str(ROOT / "bin" / "cu")


TOOLS = [
    {"name": "cu_observe", "description": "Observe a macOS app with compact native AX/OCR JSON.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "all": {"type": "boolean"}}, "required": ["app"]}},
    {"name": "cu_act", "description": "Act on a snapshot-scoped cu element and verify it.", "inputSchema": {"type": "object", "properties": {"element": {"type": "string"}, "snapshot": {"type": "string"}, "verify": {"type": "boolean", "default": True}}, "required": ["element", "snapshot"]}},
    {"name": "cu_wait", "description": "Wait for a window, semantic element/value, stability, or visual change.", "inputSchema": {"type": "object", "properties": {"mode": {"type": "string", "enum": ["window", "element", "value", "stable", "changed"]}, "app": {"type": "string"}, "target": {"type": "string"}, "role": {"type": "string"}, "gone": {"type": "boolean"}, "timeout_ms": {"type": "integer"}}, "required": ["mode", "app"]}},
    {"name": "cu_batch", "description": "Run a guarded line-oriented cu action batch in one process.", "inputSchema": {"type": "object", "properties": {"script": {"type": "string"}}, "required": ["script"]}},
    {"name": "cu_shot", "description": "Capture a full screen or named app window to a local PNG.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "name": {"type": "string"}}}},
]


def invoke(args, stdin=None):
    proc = subprocess.run([CU, *args], input=stdin, text=True, capture_output=True)
    output = (proc.stdout + proc.stderr).strip()
    return proc.returncode, output


def call_tool(name, arguments):
    arguments = arguments or {}
    if name == "cu_observe":
        args = ["--json", "observe", arguments["app"]]
        if arguments.get("all"):
            args.append("--all")
        return invoke(args)
    if name == "cu_act":
        args = ["--json", "act", arguments["element"], "--snapshot", arguments["snapshot"]]
        if arguments.get("verify", True) is False:
            args.append("--no-verify")
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
    raise ValueError(f"unknown tool: {name}")


def reply(message_id, result=None, error=None):
    payload = {"jsonrpc": "2.0", "id": message_id}
    payload["error" if error else "result"] = error or result
    raw = json.dumps(payload, separators=(",", ":")).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(raw)}\r\n\r\n".encode() + raw)
    sys.stdout.buffer.flush()


def main():
    while True:
        headers = {}
        line = sys.stdin.buffer.readline()
        if not line:
            return
        while line not in (b"\r\n", b"\n", b""):
            key, _, value = line.decode().partition(":")
            headers[key.lower()] = value.strip()
            line = sys.stdin.buffer.readline()
        length = int(headers.get("content-length", "0"))
        request = json.loads(sys.stdin.buffer.read(length))
        method = request.get("method")
        request_id = request.get("id")
        if method == "notifications/initialized":
            continue
        if request_id is None:
            continue
        if method == "initialize":
            reply(request_id, {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "cu", "version": "0.3.2"}})
        elif method == "tools/list":
            reply(request_id, {"tools": TOOLS})
        elif method == "tools/call":
            try:
                code, output = call_tool(request["params"]["name"], request["params"].get("arguments"))
                reply(request_id, {"content": [{"type": "text", "text": output}], "isError": code != 0})
            except Exception as exc:
                reply(request_id, error={"code": -32602, "message": str(exc)})
        else:
            reply(request_id, error={"code": -32601, "message": f"method not found: {method}"})


if __name__ == "__main__":
    main()
