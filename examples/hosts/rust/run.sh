#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

MANIFEST="${SCRIPT_DIR}/Cargo.toml"
BINARY_NAME="rust"
DEBUG_BIN="${REPO_ROOT}/target/debug/${BINARY_NAME}"
RELEASE_BIN="${REPO_ROOT}/target/release/${BINARY_NAME}"

if [[ -f "${RELEASE_BIN}" ]]; then
    HOST_BIN="${RELEASE_BIN}"
    LIB_DIR="${REPO_ROOT}/target/release"
elif [[ -f "${DEBUG_BIN}" ]]; then
    HOST_BIN="${DEBUG_BIN}"
    LIB_DIR="${REPO_ROOT}/target/debug"
else
    cargo build --manifest-path "${MANIFEST}"
    HOST_BIN="${DEBUG_BIN}"
    LIB_DIR="${REPO_ROOT}/target/debug"
fi

export LD_LIBRARY_PATH="${LIB_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_PRELOAD="${LIB_DIR}/libpolyplug.so${LD_PRELOAD:+:${LD_PRELOAD}}"
export POLYPLUG_PLUGIN_PATH="${POLYPLUG_PLUGIN_PATH:-${REPO_ROOT}/examples/plugins}"

exec "${HOST_BIN}" "$@"
