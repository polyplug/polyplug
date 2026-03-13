#!/usr/bin/env bash
# examples/guests/lua/build.sh — Verify Lua guest plugins
#
# Lua is interpreted — no compilation needed.
# This script validates syntax and reports each guest as ready.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    transformer
    validator
)

echo "--- [lua] Verifying Lua guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    echo "  Checking lua/${guest} ..."
    # Syntax-check all .lua files in this guest
    find "${guest_dir}" -name "*.lua" | while read -r luafile; do
        luac -p "${luafile}"
    done
    echo "  OK: lua/${guest} (no build required — interpreted)"
done

echo "--- [lua] Done ---"
