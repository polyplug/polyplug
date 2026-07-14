#!/usr/bin/env bash
# build_all.sh — rebuilds all pre-compiled test fixtures
# Run this after making changes to fixture source code.
#
# Rust fixtures are workspace members; this script builds them in --release and
# copies each produced cdylib to every location the tests consume it from:
#   - the fixtures root  (tests/fixtures/lib<name>.so), read via the *_SO env
#     vars set by crates/polyplug/build.rs
#   - the bundle subdir  (tests/fixtures/<dir>/lib<name>.so) that pairs the .so
#     with a manifest.toml, read via the *_DIR env vars
#
# Rust build failures are fatal (set -e). The C# step is tolerated because its
# toolchain may be unavailable, but its failure is reported clearly.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "Rebuilding test fixtures from ${WORKSPACE_ROOT}"

# Platform-specific cdylib extension (matches crates/polyplug/build.rs).
# Rust cdylib naming: `<name>.dll` on Windows (no `lib` prefix),
# `lib<name>.dylib` on macOS, `lib<name>.so` on Linux. Mirror exactly what the
# *_SO env vars in crates/polyplug/build.rs expect.
case "$(uname -s)" in
    Darwin) LIB_EXT="dylib" ; LIB_PREFIX="lib" ;;
    MINGW* | MSYS* | CYGWIN*) LIB_EXT="dll" ; LIB_PREFIX="" ;;
    *) LIB_EXT="so" ; LIB_PREFIX="lib" ;;
esac

RELEASE_DIR="${WORKSPACE_ROOT}/target/release"

# Rust fixture plugins (workspace members). Each entry is "<package>:<bundle_dir>".
# An empty <bundle_dir> means the plugin has no manifest.toml subdir copy.
RUST_FIXTURES=(
    "test_plugin:test_plugin_dir"
    "memory_plugin:"
    "error_plugin:"
    "reload_plugin_v1:reload_plugin_v1"
    "reload_plugin_v2:reload_plugin_v2"
    "depender_plugin:depender_plugin"
    "no_init_plugin:no_init_plugin"
    "old_abi_plugin:old_abi_plugin"
    "register_fail_plugin:register_fail_plugin"
    "cross_caller_plugin:cross_caller_plugin"
    "cross_target_plugin:cross_target_plugin"
    "cross_target_plugin_v2:cross_target_plugin_v2"
)

echo "Building Rust fixture plugins (--release)..."
CARGO_BUILD_ARGS=()
for entry in "${RUST_FIXTURES[@]}"; do
    CARGO_BUILD_ARGS+=("-p" "${entry%%:*}")
done
cargo build --release --manifest-path "${WORKSPACE_ROOT}/Cargo.toml" "${CARGO_BUILD_ARGS[@]}"

echo "Installing Rust fixture cdylibs..."
for entry in "${RUST_FIXTURES[@]}"; do
    pkg="${entry%%:*}"
    bundle_dir="${entry#*:}"
    lib_name="${LIB_PREFIX}${pkg}.${LIB_EXT}"
    src="${RELEASE_DIR}/${lib_name}"

    if [[ ! -f "${src}" ]]; then
        echo "  ERROR: expected cdylib not produced: ${src}" >&2
        exit 1
    fi

    # Fixtures root copy (consumed via the *_SO env vars).
    cp "${src}" "${SCRIPT_DIR}/${lib_name}"
    echo "  ${lib_name} -> tests/fixtures/${lib_name}"

    # Bundle subdir copy (manifest.toml + .so, consumed via the *_DIR env vars).
    if [[ -n "${bundle_dir}" ]]; then
        cp "${src}" "${SCRIPT_DIR}/${bundle_dir}/${lib_name}"
        echo "  ${lib_name} -> tests/fixtures/${bundle_dir}/${lib_name}"
    fi
done

# ─── C++ fixtures ─────────────────────────────────────────────────────────────
# libtest_plugin_cpp / libtest_plugin_cpp_throw are hand-written C++ plugins
# (sources under tests/fixtures/test_plugin_cpp/ and test_plugin_cpp_throw/).
# Each entry is "<source_dir>:<lib_basename>". g++ is required; its absence is
# fatal here because these fixtures are exercised by the C++ FFI tests.
CPP_FIXTURES=(
    "test_plugin_cpp:test_plugin_cpp"
    "test_plugin_cpp_throw:test_plugin_cpp_throw"
)

if ! command -v g++ &>/dev/null; then
    echo "  ERROR: g++ not found; required to build C++ fixtures." >&2
    exit 1
fi

echo "Building C++ fixture plugins..."
for entry in "${CPP_FIXTURES[@]}"; do
    src_dir="${entry%%:*}"
    base="${entry#*:}"
    src="${SCRIPT_DIR}/${src_dir}/${base}.cpp"
    lib_name="${LIB_PREFIX}${base}.${LIB_EXT}"
    out="${SCRIPT_DIR}/${lib_name}"

    if [[ ! -f "${src}" ]]; then
        echo "  ERROR: expected C++ source not found: ${src}" >&2
        exit 1
    fi

    # The fixtures include the real ABI SDK header (<polyplug/abi.hpp>) instead
    # of hand-rolled struct mirrors, so the SDK include path is required.
    g++ -std=c++20 -fPIC -shared -O2 \
        -I "${SCRIPT_DIR}/../../sdks/cpp/abi" \
        "${src}" -o "${out}"
    echo "  ${lib_name} -> tests/fixtures/${lib_name}"
done

# C# fixture. Tolerated because dotnet may be unavailable, but report clearly.
# Debug config: tests/integration/build.rs and crates/polyplug_dotnet/tests both
# read bin/Debug/net10.0/CsharpPlugin.dll — building any other config leaves the
# dotnet integration tests silently skipping on a clean checkout.
if command -v dotnet &>/dev/null; then
    echo "Building C# csharp_plugin..."
    if ! ( cd "${SCRIPT_DIR}/csharp_plugin" && dotnet build -c Debug ); then
        echo "  WARNING: C# csharp_plugin build failed (continuing)." >&2
    fi
else
    echo "  Skipped: dotnet not available"
fi

# ─── VM fixture provisioning ──────────────────────────────────────────────────
# The Python and Lua plugin scripts are source-only (no compile step), but they
# require their guest SDK packages at load time. The PythonLoader prepends
# <bundle_dir>/site-packages to sys.path and the LuaLoader prepends <bundle_dir>
# to package.path, so — mirroring examples/build_all.sh — each bundle SHIPS its
# dependencies inside the bundle directory. These copies are idempotent.
SDK_DIR="${WORKSPACE_ROOT}/sdks"

# Python: vendor polyplug_guest, polyplug_abi, and the polyplug.abi namespace
# (the polyplug_abi shim re-exports `from polyplug.abi.abi import *`) into the
# bundle's site-packages/ — the only extra path the PythonLoader provisions.
echo "Provisioning Python fixture site-packages..."
PY_SITE="${SCRIPT_DIR}/test_plugin_python/site-packages"
rm -rf "${PY_SITE}"
mkdir -p "${PY_SITE}/polyplug/abi"
cp -r "${SDK_DIR}/python/guest/polyplug_guest" "${PY_SITE}/polyplug_guest"
cp -r "${SDK_DIR}/python/polyplug_abi/polyplug_abi" "${PY_SITE}/polyplug_abi"
cp "${SDK_DIR}/python/abi/abi.py" "${PY_SITE}/polyplug/abi/abi.py"
: > "${PY_SITE}/polyplug/__init__.py"
: > "${PY_SITE}/polyplug/abi/__init__.py"
echo "  -> tests/fixtures/test_plugin_python/site-packages/"

# Lua: vendor polyplug_guest, polyplug_abi, and the polyplug.abi namespace (the
# polyplug_abi shim does require("polyplug.abi")) into the bundle directory,
# which the LuaLoader prepends to package.path as "<bundle_dir>/?.lua".
echo "Provisioning Lua fixture modules..."
LUA_BUNDLE="${SCRIPT_DIR}/test_plugin_lua"
rm -rf "${LUA_BUNDLE}/polyplug"
mkdir -p "${LUA_BUNDLE}/polyplug"
cp "${SDK_DIR}/lua/guest/polyplug_guest.lua" "${LUA_BUNDLE}/polyplug_guest.lua"
cp "${SDK_DIR}/lua/abi/polyplug_abi.lua" "${LUA_BUNDLE}/polyplug_abi.lua"
cp "${SDK_DIR}/lua/abi/abi.lua" "${LUA_BUNDLE}/polyplug/abi.lua"
echo "  -> tests/fixtures/test_plugin_lua/{polyplug_guest.lua,polyplug_abi.lua,polyplug/abi.lua}"

# js-quickjs fixture: bundle.js is hand-written and self-contained, no build needed.
echo "js-quickjs bundle.js is hand-written."

# ─── QuickJS GENERATED-glue fixture ───────────────────────────────────────────
# test_plugin_js/bundle.js hand-rolls the ABI; test_plugin_js_generated/ is the
# generator-output counterpart: polyplugc emits the guest glue, rolldown
# bundles it with adder.js, and integration_js_generated_guest.rs drives the
# GENERATED wrappers through a real Runtime. plugin.js + manifest.toml are
# committed, so this rebuild is tolerated (reported, not fatal) when rolldown
# is unavailable — mirroring the C# fixture policy.
JS_GEN_DIR="${SCRIPT_DIR}/test_plugin_js_generated"
if command -v rolldown >/dev/null 2>&1; then
    echo "Building QuickJS generated-glue fixture..."
    cargo build --release --manifest-path "${WORKSPACE_ROOT}/Cargo.toml" -p polyplugc
    "${RELEASE_DIR}/polyplugc" generate --bundle "${JS_GEN_DIR}/bundle.toml" \
        --lang js-quickjs --out "${JS_GEN_DIR}/generated"
    rolldown "${JS_GEN_DIR}/adder.js" --format iife --platform neutral \
        --file "${JS_GEN_DIR}/plugin.js"
    # The IIFE wraps exports, while the QuickJS loader consumes polyplug_init
    # and generated in-process callers consume the canonical manifest bytes.
    sed -i 's/^(function(exports)/var polyplug_module = (function(exports)/' "${JS_GEN_DIR}/plugin.js"
    sed -i 's/^})({});$/})({});\nglobalThis.polyplug_init = polyplug_module.polyplug_init;\nglobalThis.POLYPLUG_MANIFEST = polyplug_module.POLYPLUG_MANIFEST;/' "${JS_GEN_DIR}/plugin.js"
    cp "${JS_GEN_DIR}/generated/manifest.toml" "${JS_GEN_DIR}/manifest.toml"
    echo "  -> tests/fixtures/test_plugin_js_generated/plugin.js"
else
    echo "  WARNING: rolldown unavailable — skipping QuickJS generated-glue fixture rebuild (committed plugin.js kept)" >&2
fi

echo "Done."
