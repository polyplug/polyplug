#!/usr/bin/env bash
# examples/guests/csharp/build.sh — Build all C# guest plugins
#
# Builds each C# guest plugin using dotnet build.
# Run from anywhere; this script resolves its own directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

GUESTS=(
    encoder
    reporter
)

echo "--- [csharp] Building C# guests ---"

for guest in "${GUESTS[@]}"; do
    guest_dir="${SCRIPT_DIR}/${guest}"
    echo "  Building csharp/${guest} ..."
    (
        cd "${guest_dir}"
        dotnet build --configuration Release 2>&1
    )
    echo "  OK: csharp/${guest}"
done

echo "--- [csharp] Done ---"
