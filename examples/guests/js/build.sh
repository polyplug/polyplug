#!/usr/bin/env bash
# examples/guests/js/build.sh — Verify JavaScript guest plugins
#
# JavaScript guests use pre-bundled bundle.js — no build step needed.
# This script verifies the bundle files exist and reports each guest as ready.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    reporter
    validator
)

echo "--- [js] Verifying JS guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    bundle="${guest_dir}/bundle.js"
    echo "  Checking js/${guest} ..."
    if [[ ! -f "${bundle}" ]]; then
        echo "  ERROR: Missing bundle at ${bundle}" >&2
        exit 1
    fi
    echo "  OK: js/${guest} (no build required — pre-bundled JS)"
done

echo "--- [js] Done ---"
