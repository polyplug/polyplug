#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

LIB_DIR="${REPO_ROOT}/target/debug"
POLYPLUG_SO="${LIB_DIR}/libpolyplug.so"

if [[ ! -f "${POLYPLUG_SO}" ]]; then
    echo "ERROR: libpolyplug.so not found at ${POLYPLUG_SO}" >&2
    echo "  Build: cargo build -p polyplug" >&2
    exit 1
fi

if ! command -v deno &>/dev/null; then
    echo "ERROR: deno not found in PATH." >&2
    exit 1
fi

export LD_LIBRARY_PATH="${LIB_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export POLYPLUG_SO="${POLYPLUG_SO}"
export POLYPLUG_PLUGIN_PATH="${POLYPLUG_PLUGIN_PATH:-${REPO_ROOT}/examples/plugins}"

exec deno run --allow-read --allow-ffi --allow-env "${SCRIPT_DIR}/host.ts" "$@"
