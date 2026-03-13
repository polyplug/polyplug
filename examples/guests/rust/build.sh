#!/usr/bin/env bash
# examples/guests/rust/build.sh — Build all Rust guest plugins
#
# Builds each Rust guest plugin as a cdylib (.so) using cargo.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    decoder
    encoder
)

echo "--- [rust] Building Rust guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    echo "  Building rust/${guest} ..."
    (
        cd "${guest_dir}"
        cargo build --release 2>&1
    )
    echo "  OK: rust/${guest}"
done

echo "--- [rust] Done ---"
