#!/bin/zsh
# Preserve the historical destination override; install matching sibling helpers.
set -eu
ROOT="${0:A:h:h}"
export CU_INSTALL_DIR="${CU_MCP_INSTALL_DIR:-${CU_INSTALL_DIR:-$HOME/bin}}"
exec "$ROOT/scripts/install.zsh" "$@"
