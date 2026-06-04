#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Fail loudly with a clear message identifying the failing example/language.
fail() {
    echo "BUILD FAILED: $*" >&2
    exit 1
}

# Verify an external toolchain is present before attempting to use it.
# A missing required tool is a loud, fatal error — never a silent skip.
require_tool() {
    tool="$1"
    example="$2"
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "MISSING TOOL: $tool — cannot build $example" >&2
        exit 1
    fi
}

PLUGINS_DIR="$SCRIPT_DIR/plugins"
mkdir -p "$PLUGINS_DIR"

echo "=== Building polyplug examples ==="
echo ""

echo "[1/4] Building polyplugc..."
cargo build --release -p polyplugc || fail "polyplugc"
POLYPLUGC="$SCRIPT_DIR/../target/release/polyplugc"

echo "[2/4] Building libraries..."
cargo build --release -p polyplug -p polyplug_abi -p polyplug_guest \
    || fail "core libraries (polyplug / polyplug_abi / polyplug_guest)"

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
        "$POLYPLUGC" generate --bundle "$dir/bundle.toml" --lang "$lang" --out "$dir/generated" \
            || fail "$bundle_name (polyplugc generate)"

        bundle_dir="$PLUGINS_DIR/$bundle_name"
        mkdir -p "$bundle_dir"

        case "$lang" in
            rust)
                cargo build --release --manifest-path "$dir/Cargo.toml" \
                    || fail "$bundle_name (cargo build)"
                cp "$SCRIPT_DIR/../target/release/lib${plugin}.so" "$bundle_dir/" \
                    || fail "$bundle_name (copy lib${plugin}.so)"
                ;;
            cpp)
                require_tool g++ "$bundle_name"
                g++ -std=c++20 -fPIC -shared -O2 \
                    -I"$SCRIPT_DIR/../sdks/cpp/guest" \
                    -I"$SCRIPT_DIR/../sdks/cpp/abi" \
                    -I"$dir/generated" \
                    "$dir/$plugin.cpp" \
                    -L"$SCRIPT_DIR/../target/release" -lpolyplug \
                    -Wl,-rpath,'$ORIGIN' \
                    -o "$bundle_dir/lib$plugin.so" \
                    || fail "$bundle_name (g++ compile)"
                cp "$SCRIPT_DIR/../target/release/libpolyplug.so" "$bundle_dir/" \
                    || fail "$bundle_name (copy libpolyplug.so)"
                ;;
            python)
                cp "$dir/$plugin.py" "$bundle_dir/" || fail "$bundle_name (copy $plugin.py)"
                cp -r "$dir/generated" "$bundle_dir/" || fail "$bundle_name (copy generated)"
                # Provision SDK packages into <bundle>/site-packages/ — the only extra
                # path the PythonLoader prepends to sys.path. Mirrors the layout in
                # tests/fixtures/build_all.sh: polyplug_guest + polyplug_abi shim +
                # the polyplug.abi namespace the shim re-exports from.
                py_site="$bundle_dir/site-packages"
                rm -rf "$py_site"
                mkdir -p "$py_site/polyplug/abi"
                cp -r "$SCRIPT_DIR/../sdks/python/guest/polyplug_guest" "$py_site/polyplug_guest" \
                    || fail "$bundle_name (vendor polyplug_guest)"
                cp -r "$SCRIPT_DIR/../sdks/python/polyplug_abi/polyplug_abi" "$py_site/polyplug_abi" \
                    || fail "$bundle_name (vendor polyplug_abi)"
                cp "$SCRIPT_DIR/../sdks/python/abi/abi.py" "$py_site/polyplug/abi/abi.py" \
                    || fail "$bundle_name (vendor polyplug.abi.abi)"
                : > "$py_site/polyplug/__init__.py"
                : > "$py_site/polyplug/abi/__init__.py"
                ;;
            lua)
                cp "$dir/$plugin.lua" "$bundle_dir/" || fail "$bundle_name (copy $plugin.lua)"
                cp -r "$dir/generated" "$bundle_dir/" || fail "$bundle_name (copy generated)"
                ;;
            js-quickjs)
                require_tool rolldown "$bundle_name"
                # Bundle TypeScript or JavaScript source with rolldown
                # Both .ts and .js files use ES module syntax and need bundling to IIFE
                if [[ -f "$dir/$plugin.ts" ]]; then
                    rolldown "$dir/$plugin.ts" --format iife --platform neutral --file "$bundle_dir/$plugin.js" \
                        || fail "$bundle_name (rolldown $plugin.ts)"
                elif [[ -f "$dir/$plugin.js" ]]; then
                    rolldown "$dir/$plugin.js" --format iife --platform neutral --file "$bundle_dir/$plugin.js" \
                        || fail "$bundle_name (rolldown $plugin.js)"
                else
                    fail "$bundle_name (no $plugin.ts or $plugin.js source found)"
                fi
                # The IIFE wraps everything in an exports object, but the loader expects
                # polyplug_init to be in the global scope. Replace the IIFE with a named
                # variable assignment and extract polyplug_init to globalThis.
                sed -i 's/^(function(exports)/var polyplug_module = (function(exports)/' "$bundle_dir/$plugin.js" \
                    || fail "$bundle_name (sed rewrite IIFE)"
                sed -i 's/^})({});$/})({});\nglobalThis.polyplug_init = polyplug_module.polyplug_init;/' "$bundle_dir/$plugin.js" \
                    || fail "$bundle_name (sed export polyplug_init)"
                ;;
        esac

        # Copy generated manifest (not handwritten!)
        cp "$dir/generated/manifest.toml" "$bundle_dir/" || fail "$bundle_name (copy manifest.toml)"
    done
done

# [3.5/4] Generate HOST code with host contracts
echo "[3.5/4] Generating host code with host contracts..."
for lang in rust cpp csharp python lua js-quickjs; do
    # Map lang to directory name (js-quickjs -> js)
    lang_dir="$lang"
    if [ "$lang" = "js-quickjs" ]; then
        lang_dir="js"
    fi
    host_dir="hosts/$lang_dir"
    if [ -d "$host_dir" ]; then
        echo "  generating: $lang_dir host"
        "$POLYPLUGC" generate --api api.toml --lang "$lang" --out "$host_dir/generated" \
            || fail "$lang_dir host (polyplugc generate)"
    fi
done
echo ""

echo "[4/4] Building hosts..."

cargo build --release --manifest-path "hosts/rust/Cargo.toml" || fail "rust host (cargo build)"

require_tool g++ "cpp host"
make -C "hosts/cpp" || fail "cpp host (make)"

require_tool dotnet "csharp host"
dotnet build -c Release "hosts/csharp/Host.csproj" || fail "csharp host (dotnet build)"

echo ""
echo "=== Build complete ==="
echo ""
echo "Plugins: $PLUGINS_DIR"
ls -la "$PLUGINS_DIR"
echo ""
echo "Run hosts:"
echo "  Rust:  ./hosts/rust/target/release/pipeline_host"
