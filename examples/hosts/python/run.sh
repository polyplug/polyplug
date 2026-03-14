#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

LIB_DIR="${REPO_ROOT}/target/debug"
POLYPLUG_SO="${LIB_DIR}/libpolyplug.so"
PYTHON_HOST_LIB="${REPO_ROOT}/host-libs/python"

if [[ ! -f "${POLYPLUG_SO}" ]]; then
    echo "error: libpolyplug.so not found at ${POLYPLUG_SO}" >&2
    echo "       Build: cargo build -p polyplug" >&2
    exit 1
fi

if [[ ! -d "${PYTHON_HOST_LIB}/polyplug" ]]; then
    echo "error: polyplug Python module not found at ${PYTHON_HOST_LIB}" >&2
    exit 1
fi

export PYTHONPATH="${PYTHON_HOST_LIB}${PYTHONPATH:+:${PYTHONPATH}}"
export POLYPLUG_LIB="${POLYPLUG_SO}"
export LD_LIBRARY_PATH="${LIB_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_PRELOAD="${POLYPLUG_SO}${LD_PRELOAD:+:${LD_PRELOAD}}"
export POLYPLUG_PLUGIN_PATH="${POLYPLUG_PLUGIN_PATH:-${REPO_ROOT}/examples/plugins}"

exec python3 "${SCRIPT_DIR}/host.py" "$@"
