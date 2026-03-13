#!/usr/bin/env bash
# examples/guests/cpp/build.sh — Build all C++ guest plugins
#
# Builds each C++ guest plugin as a shared library (.so) using make.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    transformer
    validator
)

echo "--- [cpp] Building C++ guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    echo "  Building cpp/${guest} ..."
    (
        cd "${guest_dir}"
        make 2>&1
    )
    echo "  OK: cpp/${guest}"
done

echo "--- [cpp] Done ---"
