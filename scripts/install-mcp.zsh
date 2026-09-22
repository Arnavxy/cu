#!/bin/zsh
set -eu

ROOT="${0:A:h:h}"
DEST="${CU_MCP_INSTALL_DIR:-$HOME/bin}"
mkdir -p "$DEST"
install -m 755 "$ROOT/scripts/cu-mcp.py" "$DEST/cu-mcp"
print -r -- "installed optional cu MCP adapter to $DEST/cu-mcp"
