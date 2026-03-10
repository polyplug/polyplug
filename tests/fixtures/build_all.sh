#!/usr/bin/env bash
# build_all.sh — rebuilds all pre-compiled test fixtures
# Run this after making changes to fixture source code
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "Rebuilding test fixtures from ${WORKSPACE_ROOT}"

# Rust plugin fixture
echo "Building Rust test_plugin..."
cargo build -p test_plugin --release --manifest-path "${WORKSPACE_ROOT}/Cargo.toml" 2>&1 || echo "  Skipped: test_plugin not found"

# C++ fixtures (compiled by build.rs automatically via cc crate)
echo "C++ fixtures are compiled by crates/polyplug/build.rs automatically during cargo build."

# C# fixture
if command -v dotnet &>/dev/null; then
    echo "Building C# csharp_plugin..."
    cd "${SCRIPT_DIR}/csharp_plugin" && dotnet build -c Release 2>&1
else
    echo "  Skipped: dotnet not available"
fi

# Python and Lua: source-only, no build needed
echo "Python (.py) and Lua (.lua) fixtures are source-only, no build required."

# js-quickjs fixture: bundle.js is hand-written, no build needed
echo "js-quickjs bundle.js is hand-written."

# js-deno fixture: index.ts loaded natively by deno_core, no build needed
echo "js-deno index.ts is loaded natively by deno_core."

echo "Done."
