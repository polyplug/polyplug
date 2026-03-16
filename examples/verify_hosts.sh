#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

export POLYPLUG_PLUGIN_PATH="$SCRIPT_DIR/plugins"

echo "=== polyplug Examples Verification ==="
echo ""

# Run Rust host
echo "=== Rust Host ==="
"$WORKSPACE_DIR/target/release/pipeline_host" 2>&1

echo ""
echo "=== Verification Complete ==="
