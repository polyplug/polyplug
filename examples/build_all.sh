#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PLUGINS_DIR="$SCRIPT_DIR/plugins"
mkdir -p "$PLUGINS_DIR"

echo "=== Building polyplug examples ==="
echo ""

echo "[1/4] Building polyplugc..."
cargo build --release -p polyplugc
POLYPLUGC="$SCRIPT_DIR/../target/release/polyplugc"

echo "[2/4] Building libraries..."
cargo build --release -p polyplug -p polyplug_abi -p polyplug_guest

rm -rf "$PLUGINS_DIR"/*

echo "[3/4] Building guest plugins..."

LANGS="rust cpp python lua js-quickjs"
PLUGINS="decoder encoder transformer reporter validator"

for lang in $LANGS; do
    for plugin in $PLUGINS; do
        # Map lang to directory name (js-quickjs -> js)
        lang_dir="$lang"
        if [ "$lang" = "js-quickjs" ]; then
            lang_dir="js"
        fi
        
        dir="guests/$lang_dir/$plugin"
        if [ ! -d "$dir" ]; then
            continue
        fi

        # Use fixed naming convention: {lang}_{plugin}
        bundle_name="${lang_dir}_${plugin}"
        echo "  building: $bundle_name"

        # Generate code and manifest
        "$POLYPLUGC" generate --bundle "$dir/bundle.toml" --lang "$lang" --out "$dir/generated" 2>/dev/null || true

        bundle_dir="$PLUGINS_DIR/$bundle_name"
        mkdir -p "$bundle_dir"

        case "$lang" in
            rust)
                cargo build --release --manifest-path "$dir/Cargo.toml" 2>/dev/null || true
                cp "$SCRIPT_DIR/../target/release/lib${plugin}.so" "$bundle_dir/" 2>/dev/null || true
                ;;
            cpp)
                g++ -std=c++20 -fPIC -shared -O2 \
                    -I"$SCRIPT_DIR/../sdks/cpp/guest" \
                    -I"$dir/generated" \
                    "$dir/$plugin.cpp" \
                    -L"$SCRIPT_DIR/../target/release" -lpolyplug \
                    -Wl,-rpath,'$ORIGIN' \
                    -o "$bundle_dir/lib$plugin.so" 2>/dev/null || true
                cp "$SCRIPT_DIR/../target/release/libpolyplug.so" "$bundle_dir/" 2>/dev/null || true
                ;;
            python)
                cp "$dir/$plugin.py" "$bundle_dir/"
                cp -r "$dir/generated" "$bundle_dir/"
                mkdir -p "$bundle_dir/polyplug_guest"
                cp "$SCRIPT_DIR/../sdks/python/guest/polyplug_guest/"*.py "$bundle_dir/polyplug_guest/"
                ;;
            lua)
                cp "$dir/$plugin.lua" "$bundle_dir/"
                cp -r "$dir/generated" "$bundle_dir/"
                ;;
            js-quickjs)
                # Bundle TypeScript or JavaScript source with rolldown
                # Both .ts and .js files use ES module syntax and need bundling to IIFE
                if [[ -f "$dir/$plugin.ts" ]]; then
                    rolldown "$dir/$plugin.ts" --format iife --platform neutral --file "$bundle_dir/$plugin.js" 2>/dev/null || true
                elif [[ -f "$dir/$plugin.js" ]]; then
                    rolldown "$dir/$plugin.js" --format iife --platform neutral --file "$bundle_dir/$plugin.js" 2>/dev/null || true
                fi
                # The IIFE wraps everything in an exports object, but the loader expects
                # polyplug_init to be in the global scope. Replace the IIFE with a named
                # variable assignment and extract polyplug_init to globalThis.
                sed -i 's/^(function(exports)/var polyplug_module = (function(exports)/' "$bundle_dir/$plugin.js"
                sed -i 's/^})({});$/})({});\nglobalThis.polyplug_init = polyplug_module.polyplug_init;/' "$bundle_dir/$plugin.js"
                ;;
        esac

        # Copy generated manifest (not handwritten!)
        cp "$dir/generated/manifest.toml" "$bundle_dir/"
    done
done

echo ""
echo "[4/4] Building hosts..."

cargo build --release --manifest-path "hosts/rust/Cargo.toml" 2>/dev/null || true

echo ""
echo "=== Build complete ==="
echo ""
echo "Plugins: $PLUGINS_DIR"
ls -la "$PLUGINS_DIR" 2>/dev/null || echo "(no plugins built)"
echo ""
echo "Run hosts:"
echo "  Rust:  ./hosts/rust/target/release/pipeline_host"