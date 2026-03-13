#!/usr/bin/env bash
set -euo pipefail

# Find repo root (3 levels up from this script: examples/hosts/rust/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

MANIFEST="${SCRIPT_DIR}/Cargo.toml"
BINARY_NAME="rust"
DEBUG_BIN="${REPO_ROOT}/target/debug/${BINARY_NAME}"
RELEASE_BIN="${REPO_ROOT}/target/release/${BINARY_NAME}"

# Determine which binary to use; prefer release if it exists and is newer
if [[ -f "${RELEASE_BIN}" ]]; then
    HOST_BIN="${RELEASE_BIN}"
    LIB_DIR="${REPO_ROOT}/target/release"
elif [[ -f "${DEBUG_BIN}" ]]; then
    HOST_BIN="${DEBUG_BIN}"
    LIB_DIR="${REPO_ROOT}/target/debug"
else
    echo "[run.sh] Binary not found — building in debug mode..."
    cargo build --manifest-path "${MANIFEST}"
    HOST_BIN="${DEBUG_BIN}"
    LIB_DIR="${REPO_ROOT}/target/debug"
fi

# Export LD_LIBRARY_PATH so libpolyplug.so and sibling loader libraries are found
export LD_LIBRARY_PATH="${LIB_DIR}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"

echo "[run.sh] Using binary : ${HOST_BIN}"
echo "[run.sh] LD_LIBRARY_PATH: ${LD_LIBRARY_PATH}"
echo ""

exec "${HOST_BIN}" "$@"
