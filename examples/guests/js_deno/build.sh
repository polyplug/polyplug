#!/usr/bin/env bash
# examples/guests/js_deno/build.sh — Verify JS/Deno guest plugins
#
# Deno guests use pre-written index.ts — no build step needed.
# This script verifies the source files exist and reports each guest as ready.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    transformer
    reporter
)

echo "--- [js_deno] Verifying JS/Deno guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    src="${guest_dir}/index.ts"
    echo "  Checking js_deno/${guest} ..."
    if [[ ! -f "${src}" ]]; then
        echo "  ERROR: Missing source at ${src}" >&2
        exit 1
    fi
    echo "  OK: js_deno/${guest} (no build required — Deno TypeScript)"
done

echo "--- [js_deno] Done ---"
