#!/usr/bin/env python3
"""Compatibility launcher for old MCP configurations; the server is Rust."""
import os
import sys
from pathlib import Path
root = Path(__file__).resolve().parent.parent
binary = root / 'cu-runtime/target/release/cu-mcp'
if not binary.is_file():
    raise SystemExit('Build CU first: cargo build --locked --release --manifest-path cu-runtime/Cargo.toml')
os.execv(str(binary), [str(binary), *sys.argv[1:]])
