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
if command -v deno &> /dev/null && [ -f "hosts/js/host.js" ]; then
    if deno run --allow-read --allow-ffi --allow-env hosts/js/host.js 2>&1; then
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
    if LD_LIBRARY_PATH="$WORKSPACE_DIR/target/release/deps" hosts/cpp/host 2>&1; then
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

# Check for pipeline output from each host
PIPELINE_OK=0

if [ -f "$WORKSPACE_DIR/target/release/pipeline_host" ]; then
    OUTPUT=$("$WORKSPACE_DIR/target/release/pipeline_host" 2>&1)
    if echo "$OUTPUT" | grep -q "provides.*Decoder" && echo "$OUTPUT" | grep -q "provides.*Transformer"; then
        echo "✓ rust host: full pipeline executed"
        PIPELINE_OK=$((PIPELINE_OK + 1))
    else
        echo "✗ rust host: pipeline output missing"
    fi
fi

if command -v python3 &> /dev/null && [ -f "hosts/python/host.py" ]; then
    OUTPUT=$(PYTHONPATH="$WORKSPACE_DIR/host-libs/python" python3 hosts/python/host.py 2>&1)
    if echo "$OUTPUT" | grep -q "provides.*Decoder" && echo "$OUTPUT" | grep -q "provides.*Transformer"; then
        echo "✓ python host: full pipeline executed"
        PIPELINE_OK=$((PIPELINE_OK + 1))
    else
        echo "✗ python host: pipeline output missing"
    fi
fi

if command -v luajit &> /dev/null && [ -f "hosts/lua/host.lua" ]; then
    OUTPUT=$(LUA_PATH="$WORKSPACE_DIR/host-libs/lua/?.lua;;" luajit hosts/lua/host.lua 2>&1)
    if echo "$OUTPUT" | grep -q "provides.*Decoder" && echo "$OUTPUT" | grep -q "provides.*Transformer"; then
        echo "✓ lua host: full pipeline executed"
        PIPELINE_OK=$((PIPELINE_OK + 1))
    else
        echo "✗ lua host: pipeline output missing"
    fi
fi

if command -v deno &> /dev/null && [ -f "hosts/js/host.js" ]; then
    OUTPUT=$(deno run --allow-read --allow-ffi --allow-env hosts/js/host.js 2>&1)
    if echo "$OUTPUT" | grep -q "provides.*Decoder" && echo "$OUTPUT" | grep -q "provides.*Transformer"; then
        echo "✓ javascript host: full pipeline executed"
        PIPELINE_OK=$((PIPELINE_OK + 1))
    else
        echo "✗ javascript host: pipeline output missing"
    fi
fi

if command -v dotnet &> /dev/null && [ -f "hosts/csharp/Host.csproj" ]; then
    OUTPUT=$(cd hosts/csharp && dotnet run 2>&1)
    if echo "$OUTPUT" | grep -q "provides.*Decoder" && echo "$OUTPUT" | grep -q "provides.*Transformer"; then
        echo "✓ csharp host: full pipeline executed"
        PIPELINE_OK=$((PIPELINE_OK + 1))
    else
        echo "✗ csharp host: pipeline output missing"
    fi
fi

if [ -f "hosts/cpp/host" ]; then
    OUTPUT=$(LD_LIBRARY_PATH="$WORKSPACE_DIR/target/release/deps" hosts/cpp/host 2>&1)
    if echo "$OUTPUT" | grep -q "provides.*Decoder" && echo "$OUTPUT" | grep -q "provides.*Transformer"; then
        echo "✓ cpp host: full pipeline executed"
        PIPELINE_OK=$((PIPELINE_OK + 1))
    else
        echo "✗ cpp host: pipeline output missing"
    fi
fi

if [ $FAILED -eq 0 ] && [ $PIPELINE_OK -gt 0 ]; then
    echo "✓ All available hosts passed with full pipeline execution!"
    exit 0
else
    echo "✗ $FAILED host(s) failed, $PIPELINE_OK executed full pipeline"
    exit 1
fi
