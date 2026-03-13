#!/usr/bin/env bash
# Run the polyplug Lua host example.
# Sets LD_LIBRARY_PATH so libpolyplug.so (and companion loader libs) are found,
# then executes host.lua with luajit (required — standard Lua lacks the ffi module).

set -euo pipefail

# Resolve the directory containing this script, then the repo root.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

# ---------------------------------------------------------------------------
# Locate libpolyplug.so
# Priority:
#   1. POLYPLUG_SO env var (respected by host.lua as well)
#   2. Companion cdylib built from this directory's Cargo.toml
#   3. Workspace debug build
# ---------------------------------------------------------------------------
if [[ -z "${POLYPLUG_SO:-}" ]]; then
    COMPANION_SO="${SCRIPT_DIR}/target/debug/libpolyplug_lua_host.so"
    WORKSPACE_SO="${REPO_ROOT}/target/debug/libpolyplug.so"

    if [[ -f "${COMPANION_SO}" ]]; then
        export POLYPLUG_SO="${COMPANION_SO}"
    elif [[ -f "${WORKSPACE_SO}" ]]; then
        export POLYPLUG_SO="${WORKSPACE_SO}"
    else
        echo "ERROR: libpolyplug.so not found." >&2
        echo "  Build the companion lib:  cargo build --manifest-path '${SCRIPT_DIR}/Cargo.toml'" >&2
        echo "  Or build the workspace:   cargo build --manifest-path '${REPO_ROOT}/Cargo.toml'" >&2
        exit 1
    fi
fi

# Add the directory containing the chosen .so to LD_LIBRARY_PATH so the
# dynamic linker can find it and any sibling loader libs (libpolyplug_*.so).
SO_DIR="$(dirname "${POLYPLUG_SO}")"
export LD_LIBRARY_PATH="${SO_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_PRELOAD="${POLYPLUG_SO}${LD_PRELOAD:+:${LD_PRELOAD}}"

# ---------------------------------------------------------------------------
# Locate luajit (required for FFI support).
# Falls back to lua if luajit is not available, but warns the user.
# ---------------------------------------------------------------------------
if command -v luajit >/dev/null 2>&1; then
    LUA_BIN="luajit"
elif command -v lua >/dev/null 2>&1; then
    echo "WARNING: luajit not found, falling back to lua." >&2
    echo "  Standard Lua lacks the ffi module — host.lua will likely fail." >&2
    echo "  Install LuaJIT: https://luajit.org/" >&2
    LUA_BIN="lua"
else
    echo "ERROR: neither luajit nor lua found in PATH." >&2
    echo "  Install LuaJIT: https://luajit.org/" >&2
    exit 1
fi

exec "${LUA_BIN}" "${SCRIPT_DIR}/host.lua" "$@"
