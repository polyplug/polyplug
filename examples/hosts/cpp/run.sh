#!/usr/bin/env bash
set -euo pipefail

# Resolve the directory containing this script (no absolute paths)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

# Set LD_LIBRARY_PATH so libpolyplug.so is found at runtime
export LD_LIBRARY_PATH="${REPO_ROOT}/target/debug${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"

# Build the host binary if it doesn't exist yet
if [ ! -f "${SCRIPT_DIR}/polyplug_host_cpp" ]; then
    echo "[run.sh] Binary not found — running make..."
    make -C "${SCRIPT_DIR}" polyplug_host_cpp
fi

# Run the host, forwarding any arguments passed to this script
exec "${SCRIPT_DIR}/polyplug_host_cpp" "$@"
