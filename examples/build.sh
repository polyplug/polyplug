#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

POLYPLUGC="../../target/release/polyplugc"
PLUGINS_DIR="$SCRIPT_DIR/plugins"

echo "=== Building polyplug examples ==="
echo ""

# Ensure polyplugc is built
echo "[1/4] Building polyplugc..."
cargo build --release -p polyplugc 2>/dev/null

# Clean and create plugins directory
rm -rf "$PLUGINS_DIR"
mkdir -p "$PLUGINS_DIR"

echo "[2/4] Building Rust guest plugins..."

# Build all Rust guest plugins
for plugin in decoder encoder transformer reporter validator; do
    dir="guests/rust/$plugin"
    echo "  building: $plugin"
    
    # Generate code with polyplugc
    "$POLYPLUGC" generate --bundle "$dir/bundle.toml" --lang rust --out "$dir/generated" 2>/dev/null
    
    # Build the plugin
    cargo build --release --manifest-path "$dir/Cargo.toml" 2>/dev/null
    
    # Install to plugins directory
    bundle_name="rust_$plugin"
    plugin_dir="$PLUGINS_DIR/$bundle_name"
    mkdir -p "$plugin_dir"
    cp "target/release/lib${plugin}.so" "$plugin_dir/"
    
    # Generate manifest
    contract=$(grep 'contracts' "$dir/bundle.toml" | sed 's/.*\["//' | sed 's/"].*//' | sed 's/@1\.0$/@1/')
    cat > "$plugin_dir/manifest.toml" <<MANIFEST
bundle_name = "$bundle_name"
runtime = "native"
version = "1.0.0"
file = "lib${plugin}.so"
provides = ["$(grep 'contracts' "$dir/bundle.toml" | sed 's/.*\["//' | sed 's/"].*//')"]

[function_count]
"$contract" = 1
MANIFEST
done

echo "  installed $(ls -d $PLUGINS_DIR/*/ 2>/dev/null | wc -l) plugins"

echo "[3/4] Building host applications..."

# Build Rust host
cargo build --release --manifest-path hosts/rust/Cargo.toml 2>/dev/null && echo "  ✓ rust host" || echo "  ✗ rust host"

# Build C++ host (if sources exist)
if [ -f "hosts/cpp/main.cpp" ]; then
    echo "  - cpp host (requires manual build)"
fi

# Build C# host (if sources exist)
if [ -f "hosts/csharp/Program.cs" ]; then
    echo "  - csharp host (requires dotnet build)"
fi

# Build Python host (script, no build needed)
if [ -f "hosts/python/host.py" ]; then
    echo "  ✓ python host (script)"
fi

# Build Lua host (script, no build needed)
if [ -f "hosts/lua/host.lua" ]; then
    echo "  ✓ lua host (script)"
fi

# Build JS/Deno host (script, no build needed)
if [ -f "hosts/js/host.ts" ]; then
    echo "  ✓ js host (script)"
fi

echo ""
echo "[4/4] Build complete!"
echo ""
echo "Plugins installed to: $PLUGINS_DIR"
echo ""
echo "Run verification:"
echo "  ./verify_hosts.sh"
