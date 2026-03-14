#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

LIB_DIR="${REPO_ROOT}/target/debug"
POLYPLUG_SO="${LIB_DIR}/libpolyplug.so"

if [ ! -f "${POLYPLUG_SO}" ]; then
    echo "ERROR: libpolyplug.so not found at ${POLYPLUG_SO}" >&2
    echo "  Build: cargo build -p polyplug" >&2
    exit 1
fi

export LD_LIBRARY_PATH="${LIB_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_PRELOAD="${POLYPLUG_SO}${LD_PRELOAD:+:${LD_PRELOAD}}"
export POLYPLUG_PLUGIN_PATH="${POLYPLUG_PLUGIN_PATH:-${REPO_ROOT}/examples/plugins}"

if [ ! -f "${SCRIPT_DIR}/polyplug_host_cpp" ]; then
    make -C "${SCRIPT_DIR}" polyplug_host_cpp
fi

exec "${SCRIPT_DIR}/polyplug_host_cpp" "$@"
