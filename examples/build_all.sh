#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PLUGINS_DIR="$SCRIPT_DIR/plugins"
mkdir -p "$PLUGINS_DIR"

echo "=== Building polyplug examples ==="
echo ""

# Build polyplugc first
echo "[1/4] Building polyplugc..."
cargo build --release -p polyplugc
POLYPLUGC="$SCRIPT_DIR/../../target/release/polyplugc"

# Build core libraries
echo "[2/4] Building libraries..."
cargo build --release -p polyplug -p polyplug_abi -p polyplug_guest

# Clean plugins dir
rm -rf "$PLUGINS_DIR"/*

echo "[3/4] Building guest plugins..."

LANGS="rust cpp python lua js"
PLUGINS="decoder encoder transformer reporter validator"

for lang in $LANGS; do
    for plugin in $PLUGINS; do
        dir="guests/$lang/$plugin"
        bundle_name=$(grep '^name' "$dir/bundle.toml" | head -1 | cut -d'"' -f2)
        echo "  building: $bundle_name"
        
        # Generate code
        "$POLYPLUGC" generate --bundle "$dir/bundle.toml" --lang "$lang" --out "$dir/generated" 2>/dev/null || true
        
        bundle_dir="$PLUGINS_DIR/$bundle_name"
        mkdir -p "$bundle_dir"
        
        case "$lang" in
            rust)
                cargo build --release --manifest-path "$dir/Cargo.toml" 2>/dev/null || true
                cp "$dir/target/release/lib${plugin}.so" "$bundle_dir/" 2>/dev/null || true
                runtime="native"
                file="lib${plugin}.so"
                ;;
            cpp)
                g++ -std=c++17 -fPIC -shared -O2 \
                    -I"$SCRIPT_DIR/../../guest-libs/cpp" \
                    -I"$dir/generated" \
                    "$dir/$plugin.cpp" \
                    -o "$bundle_dir/lib$plugin.so" 2>/dev/null || true
                runtime="native"
                file="lib$plugin.so"
                ;;
            python)
                cp "$dir/$plugin.py" "$bundle_dir/"
                runtime="python"
                file="$plugin.py"
                ;;
            lua)
                cp "$dir/$plugin.lua" "$bundle_dir/"
                runtime="lua"
                file="$plugin.lua"
                ;;
            js)
                cp "$dir/$plugin.js" "$bundle_dir/"
                runtime="js"
                file="$plugin.js"
                ;;
        esac
        
        # Get contract from bundle.toml
        contract=$(grep 'contracts' "$dir/bundle.toml" | sed 's/.*\["//' | sed 's/"].*//')
        func_count=$([ "$contract" = "data.Reporter" ] || [ "$contract" = "data.Transformer" ] || [ "$contract" = "pipeline.Validator" ] && echo 1 || echo 1)
        
        cat > "$bundle_dir/manifest.toml" <<EOF
bundle_name = "$bundle_name"
runtime = "$runtime"
version = "1.0.0"
file = "$file"
provides = ["$contract"]

[function_count]
"$contract@1" = $func_count
EOF
    done
done

echo ""
echo "[4/4] Building hosts..."

# Build Rust host
cargo build --release --manifest-path "hosts/rust/Cargo.toml" 2>/dev/null || true

echo ""
echo "=== Build complete ==="
echo ""
echo "Plugins: $PLUGINS_DIR"
ls -la "$PLUGINS_DIR" 2>/dev/null || echo "(no plugins built)"
echo ""
echo "Run hosts:"
echo "  Rust:  ./hosts/rust/target/release/pipeline_host"
