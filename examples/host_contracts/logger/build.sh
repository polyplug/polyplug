#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$SCRIPT_DIR"

POLYPLUGC="$WORKSPACE_DIR/target/release/polyplugc"
PLUGINS_DIR="$SCRIPT_DIR/plugins"
GUEST_DIR="$SCRIPT_DIR/guest"

echo "=== Building host_contracts/logger example ==="
echo ""

# Ensure polyplugc is built
echo "[1/4] Building polyplugc..."
cargo build --release -p polyplugc 2>&1 | tail -1

# Clean and create plugins directory
rm -rf "$PLUGINS_DIR"
mkdir -p "$PLUGINS_DIR"

echo "[2/4] Generating code for all guest languages..."

# Generate code for all languages
for lang in rust cpp csharp python lua js-quickjs; do
    dir="$GUEST_DIR/$lang"
    if [ -f "$dir/bundle.toml" ]; then
        echo "  generating: $lang"
        "$POLYPLUGC" generate --bundle "$dir/bundle.toml" --lang "$lang" --out "$dir/generated"
    fi
done

echo "[3/4] Building guest plugins..."

# Build Rust plugin
echo "  building: rust_worker"
rust_dir="$GUEST_DIR/rust"
cargo build --release --manifest-path "$rust_dir/Cargo.toml" 2>&1 | tail -1
rust_plugin_dir="$PLUGINS_DIR/rust_worker"
mkdir -p "$rust_plugin_dir"
if [ "$(uname)" = "Darwin" ]; then
    cp "$rust_dir/target/release/libworker.dylib" "$rust_plugin_dir/"
elif [ "$(uname)" = "Linux" ]; then
    cp "$rust_dir/target/release/libworker.so" "$rust_plugin_dir/"
fi
cp "$rust_dir/generated/manifest.toml" "$rust_plugin_dir/"
echo "    → $rust_plugin_dir"

# Build C++ plugin
echo "  building: cpp_worker"
cpp_dir="$GUEST_DIR/cpp"
if command -v g++ &> /dev/null; then
    mkdir -p "$cpp_dir/build"
    cd "$cpp_dir/build"
    cmake .. 2>&1 | tail -1
    cmake --build . 2>&1 | tail -1
    cd "$SCRIPT_DIR"
    cpp_plugin_dir="$PLUGINS_DIR/cpp_worker"
    mkdir -p "$cpp_plugin_dir"
    if [ "$(uname)" = "Darwin" ]; then
        cp "$cpp_dir/build/libworker.dylib" "$cpp_plugin_dir/"
    elif [ "$(uname)" = "Linux" ]; then
        cp "$cpp_dir/build/libworker.so" "$cpp_plugin_dir/"
    fi
    cp "$cpp_dir/generated/manifest.toml" "$cpp_plugin_dir/"
    echo "    → $cpp_plugin_dir"
else
    echo "    ⊘ g++ not available, skipping"
fi

# Build C# plugin
echo "  building: csharp_worker"
csharp_dir="$GUEST_DIR/csharp"
if command -v dotnet &> /dev/null; then
    dotnet build --configuration Release "$csharp_dir/CsharpWorker.csproj" 2>&1 | tail -1
    csharp_plugin_dir="$PLUGINS_DIR/csharp_worker"
    mkdir -p "$csharp_plugin_dir"
    cp "$csharp_dir/bin/Release/net10.0/CsharpWorker.dll" "$csharp_plugin_dir/"
    cp "$csharp_dir/bin/Release/net10.0/CsharpWorker.pdb" "$csharp_plugin_dir/" 2>/dev/null || true
    cp "$csharp_dir/generated/manifest.toml" "$csharp_plugin_dir/"
    echo "    → $csharp_plugin_dir"
else
    echo "    ⊘ dotnet not available, skipping"
fi

# Python plugin (no build, just copy)
echo "  building: python_worker"
python_dir="$GUEST_DIR/python"
python_plugin_dir="$PLUGINS_DIR/python_worker"
mkdir -p "$python_plugin_dir"
cp "$python_dir/plugin.py" "$python_plugin_dir/"
cp "$python_dir/generated/manifest.toml" "$python_plugin_dir/"
echo "    → $python_plugin_dir"

# Lua plugin (no build, just copy)
echo "  building: lua_worker"
lua_dir="$GUEST_DIR/lua"
lua_plugin_dir="$PLUGINS_DIR/lua_worker"
mkdir -p "$lua_plugin_dir"
cp "$lua_dir/plugin.lua" "$lua_plugin_dir/"
cp "$lua_dir/generated/manifest.toml" "$lua_plugin_dir/"
echo "    → $lua_plugin_dir"

# JavaScript plugin (no build, just copy)
echo "  building: js_worker"
js_dir="$GUEST_DIR/js-quickjs"
js_plugin_dir="$PLUGINS_DIR/js_worker"
mkdir -p "$js_plugin_dir"
cp "$js_dir/plugin.js" "$js_plugin_dir/"
cp "$js_dir/generated/manifest.toml" "$js_plugin_dir/"
echo "    → $js_plugin_dir"

echo "[4/4] Verifying plugins..."
plugin_count=$(ls -d "$PLUGINS_DIR"/*/ 2>/dev/null | wc -l)
echo "  installed $plugin_count plugins in $PLUGINS_DIR"
for dir in "$PLUGINS_DIR"/*/; do
    if [ -d "$dir" ]; then
        name=$(basename "$dir")
        files=$(ls "$dir" | tr '\n' ' ')
        echo "    - $name: $files"
    fi
done

echo ""
echo "=== Build complete ==="
echo ""
echo "Run with:"
echo "  cd $SCRIPT_DIR/host/rust && POLYPLUG_PLUGIN_PATH=$PLUGINS_DIR cargo run --release"
