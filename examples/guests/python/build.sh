#!/usr/bin/env bash
# examples/guests/python/build.sh — Verify Python guest plugins
#
# Python is interpreted — no compilation needed.
# This script validates syntax and reports each guest as ready.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    decoder
    reporter
)

echo "--- [python] Verifying Python guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    echo "  Checking python/${guest} ..."
    # Syntax-check all .py files in this guest
    find "${guest_dir}" -name "*.py" | while read -r pyfile; do
        python3 -m py_compile "${pyfile}"
    done
    echo "  OK: python/${guest} (no build required — interpreted)"
done

echo "--- [python] Done ---"
