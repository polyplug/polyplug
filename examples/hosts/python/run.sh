#!/usr/bin/env bash
set -euo pipefail

# Resolve repo root as two directories above this script's directory
# (examples/hosts/python/ → examples/hosts/ → examples/ → repo root)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

POLYPLUG_LIB="${REPO_ROOT}/target/release"
PYTHON_HOST_LIB="${REPO_ROOT}/host-libs/python"

if [[ ! -f "${POLYPLUG_LIB}/libpolyplug.so" ]]; then
    echo "error: libpolyplug.so not found at ${POLYPLUG_LIB}" >&2
    echo "       Build it first: cargo build --release -p polyplug" >&2
    exit 1
fi

if [[ ! -d "${PYTHON_HOST_LIB}/polyplug" ]]; then
    echo "error: polyplug Python module not found at ${PYTHON_HOST_LIB}" >&2
    exit 1
fi

export PYTHONPATH="${PYTHON_HOST_LIB}${PYTHONPATH:+:${PYTHONPATH}}"
export LD_LIBRARY_PATH="${POLYPLUG_LIB}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_PRELOAD="${POLYPLUG_LIB}/libpolyplug.so${LD_PRELOAD:+:${LD_PRELOAD}}"

exec python3 "${SCRIPT_DIR}/host.py" "$@"
