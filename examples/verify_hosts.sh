#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

export POLYPLUG_PLUGIN_PATH="$WORKSPACE_DIR/examples/plugins"
export LD_LIBRARY_PATH="$WORKSPACE_DIR/target/release/deps:${LD_LIBRARY_PATH:-}"
export POLYPLUG_LIB_PATH="$WORKSPACE_DIR/target/release/deps/libpolyplug.so"
export POLYPLUG_NATIVE_LIB_PATH="$WORKSPACE_DIR/target/release/deps/libpolyplug_native.so"

echo "=== polyplug Examples Verification ==="
echo "Library path: $WORKSPACE_DIR/target/release/deps"
echo ""

FAILED=0

# Run Rust host
echo "=== Rust Host ==="
if "$WORKSPACE_DIR/target/release/pipeline_host" 2>&1; then
    echo "✓ rust host passed"
else
    echo "✗ rust host failed"
    FAILED=$((FAILED + 1))
fi
echo ""

# Run Python host
echo "=== Python Host ==="
if command -v python3 &> /dev/null && [ -f "hosts/python/host.py" ]; then
    if PYTHONPATH="$WORKSPACE_DIR/host-libs/python" python3 hosts/python/host.py 2>&1; then
        echo "✓ python host passed"
    else
        echo "✗ python host failed"
        FAILED=$((FAILED + 1))
    fi
else
    echo "⊘ python host skipped"
fi
echo ""

# Run Lua host
echo "=== Lua Host ==="
if command -v luajit &> /dev/null && [ -f "hosts/lua/host.lua" ]; then
    if LUA_PATH="$WORKSPACE_DIR/host-libs/lua/?.lua;;" luajit hosts/lua/host.lua 2>&1; then
        echo "✓ lua host passed"
    else
        echo "✗ lua host failed"
        FAILED=$((FAILED + 1))
    fi
else
    echo "⊘ lua host skipped"
fi
echo ""

# Run JS/Deno host
echo "=== JavaScript (Deno) Host ==="
if command -v deno &> /dev/null && [ -f "hosts/js/host.ts" ]; then
    if deno run --allow-read --allow-ffi --allow-env hosts/js/host.ts 2>&1; then
        echo "✓ js host passed"
    else
        echo "✗ js host failed"
        FAILED=$((FAILED + 1))
    fi
else
    echo "⊘ js host skipped"
fi
echo ""

# Run C# host
echo "=== C# Host ==="
if command -v dotnet &> /dev/null && [ -f "hosts/csharp/Host.csproj" ]; then
    if (cd hosts/csharp && dotnet run 2>&1); then
        echo "✓ csharp host passed"
    else
        echo "✗ csharp host failed"
        FAILED=$((FAILED + 1))
    fi
else
    echo "⊘ csharp host skipped"
fi
echo ""

# Run C++ host
echo "=== C++ Host ==="
if [ -f "hosts/cpp/host" ]; then
    if ./hosts/cpp/host 2>&1; then
        echo "✓ cpp host passed"
    else
        echo "✗ cpp host failed"
        FAILED=$((FAILED + 1))
    fi
else
    echo "⊘ cpp host skipped (not built)"
fi
echo ""

echo "=== Verification Summary ==="
if [ $FAILED -eq 0 ]; then
    echo "✓ All available hosts passed!"
    exit 0
else
    echo "✗ $FAILED host(s) failed"
    exit 1
fi
