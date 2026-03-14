#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

if [[ -z "${POLYPLUG_SO:-}" ]]; then
    WORKSPACE_SO="${REPO_ROOT}/target/debug/libpolyplug.so"

    if [[ -f "${WORKSPACE_SO}" ]]; then
        export POLYPLUG_SO="${WORKSPACE_SO}"
    else
        echo "ERROR: libpolyplug.so not found at ${WORKSPACE_SO}" >&2
        echo "  Build: cargo build -p polyplug" >&2
        exit 1
    fi
fi

SO_DIR="$(dirname "${POLYPLUG_SO}")"
export LD_LIBRARY_PATH="${SO_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_PRELOAD="${POLYPLUG_SO}${LD_PRELOAD:+:${LD_PRELOAD}}"
export POLYPLUG_PLUGIN_PATH="${POLYPLUG_PLUGIN_PATH:-${REPO_ROOT}/examples/plugins}"

if command -v luajit >/dev/null 2>&1; then
    LUA_BIN="luajit"
elif command -v lua >/dev/null 2>&1; then
    echo "WARNING: luajit not found, falling back to lua." >&2
    echo "  Standard Lua lacks the ffi module — host.lua will likely fail." >&2
    LUA_BIN="lua"
else
    echo "ERROR: neither luajit nor lua found in PATH." >&2
    exit 1
fi

exec "${LUA_BIN}" "${SCRIPT_DIR}/host.lua" "$@"
